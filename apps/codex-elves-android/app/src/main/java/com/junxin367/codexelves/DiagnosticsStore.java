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
import java.util.ArrayList;
import java.util.Arrays;
import java.util.Comparator;
import java.util.Deque;
import java.util.List;
import java.util.UUID;
import java.util.concurrent.ArrayBlockingQueue;
import java.util.concurrent.RejectedExecutionException;
import java.util.concurrent.ThreadPoolExecutor;
import java.util.concurrent.TimeUnit;
import java.util.function.BooleanSupplier;

/** All file work, including snapshots and exports, stays on its own bounded IO queue. */
final class DiagnosticsStore {
    // 用户要求延长故障前日志的留存：普通日志两份各 4 MiB 轮转，主题仍独占 64 KiB，沿用独立后台 IO 队列。
    static final int MAX_FILE_BYTES = 4 * 1024 * 1024;
    static final int MAX_THEME_BYTES = 64 * 1024;
    private static final int MAX_THEME_GROUP_EVENTS = 64;
    // 上报同时受 10,000 条和 4 MiB 限制；须同步网页 diagnostics.js 与服务端 diagnostics.rs，避免中途截断或拒收。
    static final int MAX_SNAPSHOT_BYTES = 4 * 1024 * 1024;
    static final int MAX_SNAPSHOT_EVENTS = 10000;
    private final File directory;
    private final File exports;
    private final ThreadPoolExecutor io = new ThreadPoolExecutor(
        1, 1, 0, TimeUnit.MILLISECONDS, new ArrayBlockingQueue<>(128),
        runnable -> new Thread(runnable, "codex-elves-diagnostics")
    );
    private volatile Scope scope;
    private Scope preparedScope;
    private boolean closed;
    // 切换前后与日常光照各留一半容量，持续传感器回调不能淘汰刚发生的切换证据。
    private final EventWindow themeTransitions = new EventWindow(MAX_THEME_GROUP_EVENTS, MAX_THEME_BYTES / 2);
    private final EventWindow themeObservations = new EventWindow(MAX_THEME_GROUP_EVENTS, MAX_THEME_BYTES / 2);

    interface Callback<T> {
        void complete(T value, String errorCode);
    }

    DiagnosticsStore(Context context, String origin) {
        this(context.getFilesDir(), context.getCacheDir(), origin);
    }

    DiagnosticsStore(File filesDir, File cacheDir, String origin) {
        directory = new File(filesDir, "diagnostics");
        exports = new File(cacheDir, "diagnostics-exports");
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
                if (name.startsWith("theme.")) appendTheme(line);
                else append(line);
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
        // Reserve space for the fixed wrapper and app metadata as well as event separators.
        int budget = MAX_SNAPSHOT_BYTES - app.toString().getBytes(StandardCharsets.UTF_8).length - 256;
        EventWindow regular = new EventWindow(MAX_SNAPSHOT_EVENTS, budget);
        EventWindow transitions = new EventWindow(MAX_THEME_GROUP_EVENTS, MAX_THEME_BYTES / 2);
        EventWindow observations = new EventWindow(MAX_THEME_GROUP_EVENTS, MAX_THEME_BYTES / 2);
        for (File file : new File[] { previous(), current() }) {
            for (StoredEvent event : readEvents(file, MAX_FILE_BYTES)) {
                if (!event.theme) regular.add(event);
                else if (event.transition) transitions.add(event);
                else observations.add(event);
            }
        }
        for (StoredEvent event : themeTransitions.events) transitions.add(event);
        for (StoredEvent event : themeObservations.events) observations.add(event);
        // 扩大后的快照限 10,000 条 / 4 MiB；仍先保留最多 128 条主题证据，其余留给最新普通事件。
        observations.trim(MAX_THEME_GROUP_EVENTS, Math.max(0, budget - transitions.bytes));
        transitions.trim(MAX_THEME_GROUP_EVENTS, Math.max(0, budget));
        regular.trim(MAX_SNAPSHOT_EVENTS - transitions.events.size() - observations.events.size(),
            Math.max(0, budget - transitions.bytes - observations.bytes));
        List<StoredEvent> lines = new ArrayList<>(regular.events);
        lines.addAll(transitions.events);
        lines.addAll(observations.events);
        lines.sort(Comparator.comparingLong(event -> event.timeMs));
        JSONArray events = new JSONArray();
        for (StoredEvent event : lines) events.put(new JSONObject(event.line));
        return new JSONObject().put("schemaVersion", 1)
            .put("capturedAtMs", System.currentTimeMillis()).put("app", app).put("events", events);
    }

    private static StoredEvent storedEvent(String line) {
        if (line.length() > DiagnosticsPolicy.MAX_EVENT_CHARS) return null;
        try {
            JSONObject event = new JSONObject(line);
            Object time = event.opt("timeMs");
            String source = event.optString("source", "");
            boolean valid = event.length() == 4 && time instanceof Number
                && ((Number) time).longValue() >= 0
                && ("web".equals(source) || "native".equals(source))
                && event.opt("name") instanceof String
                && DiagnosticsPolicy.validName(event.optString("name"))
                && DiagnosticsPolicy.validFields(event.optJSONObject("fields"));
            return valid ? new StoredEvent(line, ((Number) time).longValue(), event.optString("name")) : null;
        } catch (JSONException ignored) {
            return null;
        }
    }

    private static List<StoredEvent> readEvents(File file, int maximumBytes) throws IOException {
        List<StoredEvent> events = new ArrayList<>();
        if (!file.isFile()) return events;
        if (file.length() > maximumBytes) {
            Files.deleteIfExists(file.toPath());
            return events;
        }
        try (BufferedReader reader = new BufferedReader(new InputStreamReader(
            new FileInputStream(file), StandardCharsets.UTF_8))) {
            String line;
            while ((line = reader.readLine()) != null) {
                StoredEvent event = storedEvent(line);
                if (event != null) events.add(event);
            }
        }
        return events;
    }

    private void appendTheme(String line) throws IOException {
        StoredEvent event = storedEvent(line);
        if (event == null) return;
        (event.transition ? themeTransitions : themeObservations).add(event);
        List<StoredEvent> retained = new ArrayList<>(themeTransitions.events);
        retained.addAll(themeObservations.events);
        retained.sort(Comparator.comparingLong(value -> value.timeMs));
        try (FileOutputStream output = new FileOutputStream(themeJournal())) {
            for (StoredEvent value : retained) output.write((value.line + "\n").getBytes(StandardCharsets.UTF_8));
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
            Files.deleteIfExists(themeJournal().toPath());
            // An exported file must never survive a logout or service change.
            clearExports();
        }
        for (File file : new File[] { current(), previous() }) {
            if (file.length() > MAX_FILE_BYTES) Files.deleteIfExists(file.toPath());
        }
        themeTransitions.clear();
        themeObservations.clear();
        for (StoredEvent event : readEvents(themeJournal(), MAX_THEME_BYTES)) {
            if (!event.theme) continue;
            (event.transition ? themeTransitions : themeObservations).add(event);
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

    private File themeJournal() {
        return new File(directory, "events-theme.jsonl");
    }

    private static final class StoredEvent {
        final String line;
        final long timeMs;
        final int bytes;
        final boolean theme;
        final boolean transition;

        StoredEvent(String line, long timeMs, String name) {
            this.line = line;
            this.timeMs = timeMs;
            bytes = line.getBytes(StandardCharsets.UTF_8).length + 1;
            theme = name.startsWith("theme.");
            transition = "theme.transition".equals(name) || "theme.sample".equals(name) || "theme.snapshot".equals(name);
        }
    }

    private static final class EventWindow {
        final Deque<StoredEvent> events = new ArrayDeque<>();
        final int maximumEvents;
        final int maximumBytes;
        int bytes;

        EventWindow(int maximumEvents, int maximumBytes) {
            this.maximumEvents = maximumEvents;
            this.maximumBytes = maximumBytes;
        }

        void add(StoredEvent event) {
            events.addLast(event);
            bytes += event.bytes;
            trim(maximumEvents, maximumBytes);
        }

        void trim(int count, int budget) {
            while (!events.isEmpty() && (events.size() > count || bytes > budget)) bytes -= events.removeFirst().bytes;
        }

        void clear() {
            events.clear();
            bytes = 0;
        }
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
