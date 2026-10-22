package com.junxin367.codexelves;

import android.content.Context;

import org.json.JSONArray;
import org.json.JSONException;
import org.json.JSONObject;

import java.io.BufferedReader;
import java.io.File;
import java.io.FileInputStream;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.InputStreamReader;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.security.MessageDigest;
import java.security.NoSuchAlgorithmException;
import java.util.ArrayDeque;
import java.util.Arrays;
import java.util.Comparator;
import java.util.Deque;
import java.util.UUID;
import java.util.concurrent.ArrayBlockingQueue;
import java.util.concurrent.RejectedExecutionException;
import java.util.concurrent.ThreadPoolExecutor;
import java.util.concurrent.TimeUnit;
import java.util.function.BooleanSupplier;

/** All file work, including snapshots and exports, stays on its own bounded IO queue. */
final class DiagnosticsStore {
    // Leave room for the 64-byte scope marker within the total 512 KiB disk bound.
    static final int MAX_FILE_BYTES = 256 * 1024 - 64;
    static final int MAX_SNAPSHOT_BYTES = 256 * 1024;
    static final int MAX_SNAPSHOT_EVENTS = 400;
    private final File directory;
    private final File exports;
    private final ThreadPoolExecutor io = new ThreadPoolExecutor(
        1, 1, 0, TimeUnit.MILLISECONDS, new ArrayBlockingQueue<>(128),
        runnable -> new Thread(runnable, "codex-elves-diagnostics")
    );
    private volatile Scope scope;
    private Scope preparedScope;
    private boolean closed;

    interface Callback<T> {
        void complete(T value, String errorCode);
    }

    DiagnosticsStore(Context context, String origin) {
        directory = new File(context.getFilesDir(), "diagnostics");
        exports = new File(context.getCacheDir(), "diagnostics-exports");
        scope = new Scope(scopeKey(origin), false);
    }

    synchronized void activateOrigin(String origin) {
        String key = scopeKey(origin);
        if (key.equals(scope.key)) return;
        scope = new Scope(key, true);
        Scope selected = scope;
        enqueue(() -> {
            if (isCurrent(selected)) {
                try {
                    prepare(selected, true);
                } catch (IOException ignored) {
                    // Diagnostics must not change the application's connection behavior.
                }
            }
        });
    }

    synchronized void clear() {
        scope = new Scope(scope.key, true);
        Scope selected = scope;
        enqueue(() -> {
            if (isCurrent(selected)) {
                try {
                    prepare(selected, true);
                } catch (IOException ignored) {
                    // A later append retries preparation; never reuse another scope's data.
                }
            }
        });
    }

    void record(String source, String name, JSONObject fields) {
        record(scope, source, name, fields);
    }

    void recordAtOrigin(String origin, String source, String name, JSONObject fields) {
        recordAtOrigin(origin, source, name, fields, () -> true);
    }

    void recordAtOrigin(
        String origin, String source, String name, JSONObject fields, BooleanSupplier current
    ) {
        Scope selected = scope;
        if (selected.key.equals(scopeKey(origin)) && current.getAsBoolean()) {
            record(selected, source, name, fields);
        }
    }

    private void record(Scope selected, String source, String name, JSONObject fields) {
        if (!("native".equals(source) || "web".equals(source))
            || !DiagnosticsPolicy.validName(name) || !DiagnosticsPolicy.validFields(fields)) {
            return;
        }
        final String line;
        try {
            line = new JSONObject()
                .put("timeMs", System.currentTimeMillis())
                .put("source", source)
                .put("name", name)
                .put("fields", fields)
                .toString();
        } catch (JSONException ignored) {
            return;
        }
        if (line.length() > DiagnosticsPolicy.MAX_EVENT_CHARS) return;
        enqueue(() -> {
            if (!isCurrent(selected)) return;
            try {
                prepare(selected, false);
                append(line);
            } catch (IOException ignored) {
                // No raw exception, response body, or filesystem path enters the log.
            }
        });
    }

    void snapshot(JSONObject app, Callback<JSONObject> callback) {
        Scope selected = scope;
        if (!enqueue(() -> {
            if (!isCurrent(selected)) {
                callback.complete(null, "connection_changed");
                return;
            }
            try {
                prepare(selected, false);
                JSONObject result = createSnapshot(app);
                callback.complete(isCurrent(selected) ? result : null,
                    isCurrent(selected) ? null : "connection_changed");
            } catch (IOException | JSONException ignored) {
                callback.complete(null, "snapshot_failed");
            }
        })) {
            callback.complete(null, "diagnostics_busy");
        }
    }

    void export(JSONObject app, Callback<File> callback) {
        Scope selected = scope;
        if (!enqueue(() -> {
            if (!isCurrent(selected)) {
                callback.complete(null, "connection_changed");
                return;
            }
            File target = null;
            try {
                prepare(selected, false);
                byte[] data = createSnapshot(app).toString().getBytes(StandardCharsets.UTF_8);
                if (!exports.isDirectory() && !exports.mkdirs()) throw new IOException();
                pruneExports();
                target = new File(exports, "CodexElves-diagnostics-"
                    + System.currentTimeMillis() + "-" + UUID.randomUUID() + ".json");
                try (FileOutputStream output = new FileOutputStream(target)) {
                    output.write(data);
                }
                if (!isCurrent(selected)) {
                    Files.deleteIfExists(target.toPath());
                    callback.complete(null, "connection_changed");
                    return;
                }
                callback.complete(target, null);
            } catch (IOException | JSONException ignored) {
                if (target != null) target.delete();
                callback.complete(null, "export_failed");
            }
        })) {
            callback.complete(null, "diagnostics_busy");
        }
    }

    private JSONObject createSnapshot(JSONObject app) throws IOException, JSONException {
        Deque<String> lines = new ArrayDeque<>();
        int retainedBytes = 0;
        // Reserve space for the fixed wrapper and app metadata as well as event separators.
        int budget = MAX_SNAPSHOT_BYTES - app.toString().getBytes(StandardCharsets.UTF_8).length - 256;
        for (File file : new File[] { previous(), current() }) {
            if (!file.isFile()) continue;
            if (file.length() > MAX_FILE_BYTES) {
                Files.deleteIfExists(file.toPath());
                continue;
            }
            try (BufferedReader reader = new BufferedReader(new InputStreamReader(
                new FileInputStream(file), StandardCharsets.UTF_8))) {
                String line;
                while ((line = reader.readLine()) != null) {
                    if (!validStoredEvent(line)) continue;
                    int bytes = line.getBytes(StandardCharsets.UTF_8).length + 1;
                    lines.addLast(line);
                    retainedBytes += bytes;
                    while (lines.size() > MAX_SNAPSHOT_EVENTS || retainedBytes > budget) {
                        retainedBytes -= lines.removeFirst().getBytes(StandardCharsets.UTF_8).length + 1;
                    }
                }
            }
        }
        JSONArray events = new JSONArray();
        for (String line : lines) events.put(new JSONObject(line));
        return new JSONObject().put("schemaVersion", 1)
            .put("capturedAtMs", System.currentTimeMillis()).put("app", app).put("events", events);
    }

    private static boolean validStoredEvent(String line) {
        if (line.length() > DiagnosticsPolicy.MAX_EVENT_CHARS) return false;
        try {
            JSONObject event = new JSONObject(line);
            Object time = event.opt("timeMs");
            String source = event.optString("source", "");
            return event.length() == 4 && time instanceof Number
                && ((Number) time).longValue() >= 0
                && ("web".equals(source) || "native".equals(source))
                && event.opt("name") instanceof String
                && DiagnosticsPolicy.validName(event.optString("name"))
                && DiagnosticsPolicy.validFields(event.optJSONObject("fields"));
        } catch (JSONException ignored) {
            return false;
        }
    }

    private void append(String line) throws IOException {
        byte[] bytes = (line + "\n").getBytes(StandardCharsets.UTF_8);
        if (bytes.length > MAX_FILE_BYTES) return;
        File active = current();
        if (active.length() + bytes.length > MAX_FILE_BYTES) {
            Files.deleteIfExists(previous().toPath());
            if (active.exists() && !active.renameTo(previous())) throw new IOException();
        }
        try (FileOutputStream output = new FileOutputStream(active, true)) {
            output.write(bytes);
        }
    }

    private void prepare(Scope selected, boolean clear) throws IOException {
        if (preparedScope == selected && !clear) return;
        if (!directory.isDirectory() && !directory.mkdirs()) throw new IOException();
        File scopeFile = new File(directory, "scope");
        String saved = scopeFile.isFile() && scopeFile.length() <= 64
            ? new String(Files.readAllBytes(scopeFile.toPath()), StandardCharsets.US_ASCII) : "";
        if (clear || selected.reset || !selected.key.equals(saved)) {
            Files.deleteIfExists(current().toPath());
            Files.deleteIfExists(previous().toPath());
            // An exported file must never survive a logout or service change.
            clearExports();
        }
        for (File file : new File[] { current(), previous() }) {
            if (file.length() > MAX_FILE_BYTES) Files.deleteIfExists(file.toPath());
        }
        try (FileOutputStream output = new FileOutputStream(scopeFile)) {
            output.write(selected.key.getBytes(StandardCharsets.US_ASCII));
        }
        preparedScope = selected;
    }

    private void clearExports() throws IOException {
        File[] files = exports.listFiles();
        if (files == null) return;
        for (File file : files) {
            if (isExport(file)) Files.deleteIfExists(file.toPath());
        }
    }

    private void pruneExports() throws IOException {
        File[] files = exports.listFiles(DiagnosticsStore::isExport);
        if (files == null) return;
        Arrays.sort(files, Comparator.comparingLong(File::lastModified).reversed());
        long expiry = System.currentTimeMillis() - TimeUnit.DAYS.toMillis(1);
        for (int index = 0; index < files.length; index++) {
            if (index >= 2 || files[index].lastModified() < expiry) {
                Files.deleteIfExists(files[index].toPath());
            }
        }
    }

    private static boolean isExport(File file) {
        return file.isFile()
            && file.getName().matches("CodexElves-diagnostics-[0-9]+-[a-f0-9-]{36}\\.json");
    }

    private File current() {
        return new File(directory, "events-current.jsonl");
    }

    private File previous() {
        return new File(directory, "events-previous.jsonl");
    }

    private boolean isCurrent(Scope selected) {
        return scope == selected;
    }

    private synchronized boolean enqueue(Runnable task) {
        if (closed) return false;
        try {
            io.execute(task);
            return true;
        } catch (RejectedExecutionException ignored) {
            return false;
        }
    }

    synchronized void close() {
        closed = true;
        // Flush already accepted lifecycle records without waiting on the UI thread.
        io.shutdown();
    }

    private static String scopeKey(String origin) {
        try {
            byte[] bytes = MessageDigest.getInstance("SHA-256").digest(
                (origin == null ? "" : origin).getBytes(StandardCharsets.UTF_8));
            StringBuilder result = new StringBuilder(64);
            for (byte value : bytes) result.append(String.format(java.util.Locale.ROOT, "%02x", value & 0xff));
            return result.toString();
        } catch (NoSuchAlgorithmException impossible) {
            throw new IllegalStateException("SHA-256 unavailable", impossible);
        }
    }

    private static final class Scope {
        final String key;
        final boolean reset;

        Scope(String key, boolean reset) {
            this.key = key;
            this.reset = reset;
        }
    }
}
