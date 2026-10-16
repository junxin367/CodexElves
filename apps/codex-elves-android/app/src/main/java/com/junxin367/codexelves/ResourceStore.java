package com.junxin367.codexelves;

import android.content.Context;
import android.content.SharedPreferences;
import android.webkit.CookieManager;

import java.io.BufferedInputStream;
import java.io.BufferedOutputStream;
import java.io.File;
import java.io.FileInputStream;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.net.HttpURLConnection;
import java.net.URL;
import java.nio.charset.StandardCharsets;
import java.security.MessageDigest;
import java.security.NoSuchAlgorithmException;
import java.util.HashSet;
import java.util.Locale;
import java.util.Set;
import java.util.UUID;
import java.util.concurrent.atomic.AtomicBoolean;
import java.util.function.BooleanSupplier;

final class ResourceStore {
    static final String BUILTIN = "builtin";
    private static final String ZROK_HEADER = "skip_zrok_interstitial";
    private static final String ZROK_VALUE = "1";
    private static final int CONNECT_TIMEOUT_MS = 15_000;
    private static final int READ_TIMEOUT_MS = 30_000;
    private static final AtomicBoolean STARTUP_CLEANUP_DONE = new AtomicBoolean();

    private final Context context;
    private final File cacheRoot;
    private final SharedPreferences preferences;

    ResourceStore(Context context) {
        this.context = context.getApplicationContext();
        this.cacheRoot = new File(context.getFilesDir(), "web-resources");
        this.preferences = context.getSharedPreferences("codex_elves_resource_state", Context.MODE_PRIVATE);
        StartupStagingCleaner.cleanupOnce(cacheRoot, STARTUP_CLEANUP_DONE);
    }

    synchronized Snapshot selectForPage(String origin) throws IOException {
        String prefix = prefix(origin);
        String unfinishedTrial = preferences.getString(prefix + "trial", "");
        String active = preferences.getString(prefix + "active", BUILTIN);
        ResourceStatePolicy.Recovery recovery = ResourceStatePolicy.recoverInterruptedTrial(
            active,
            preferences.getString(prefix + "previous", BUILTIN),
            unfinishedTrial
        );
        if (recovery.clearTrial) {
            preferences.edit()
                .putString(prefix + "active", recovery.active)
                .remove(prefix + "trial")
                .remove(prefix + "previous")
                .apply();
        }
        active = recovery.active;
        String pending = preferences.getString(prefix + "pending", "");
        if (!pending.isEmpty() && validateCached(origin, pending)) {
            preferences.edit()
                .putString(prefix + "previous", active)
                .putString(prefix + "active", pending)
                .putString(prefix + "trial", pending)
                .remove(prefix + "pending")
                .apply();
            active = pending;
        } else if (!pending.isEmpty()) {
            preferences.edit().remove(prefix + "pending").apply();
        }

        try {
            return loadSnapshot(origin, active);
        } catch (IOException activeError) {
            String previous = preferences.getString(prefix + "previous", BUILTIN);
            preferences.edit()
                .putString(prefix + "active", previous)
                .remove(prefix + "trial")
                .apply();
            return loadSnapshot(origin, previous);
        }
    }

    synchronized boolean isTrial(String origin, String version) {
        return version != null && version.equals(preferences.getString(prefix(origin) + "trial", ""));
    }

    synchronized void markReady(String origin, String version) {
        String prefix = prefix(origin);
        if (version.equals(preferences.getString(prefix + "trial", ""))) {
            preferences.edit().remove(prefix + "trial").apply();
            cleanup(origin);
        }
    }

    synchronized boolean rollbackTrial(String origin, String version) {
        String prefix = prefix(origin);
        if (!version.equals(preferences.getString(prefix + "trial", ""))) {
            return false;
        }
        String previous = preferences.getString(prefix + "previous", BUILTIN);
        preferences.edit()
            .putString(prefix + "active", previous)
            .remove(prefix + "trial")
            .remove(prefix + "previous")
            .apply();
        return true;
    }

    UpdateResult checkForUpdate(
        String origin,
        String currentVersion,
        BooleanSupplier stillCurrent
    ) {
        File staging = null;
        try {
            byte[] manifestBytes = fetch(origin, "/app-resources.json", ResourceManifest.MAX_MANIFEST_BYTES);
            ResourceManifest manifest = ResourceManifest.parse(manifestBytes);
            if (manifest.minShellVersion > BuildConfig.SHELL_VERSION) {
                return UpdateResult.unsupported();
            }
            if (manifest.version.equals(currentVersion)) {
                return UpdateResult.unchanged();
            }
            if (!stillCurrent.getAsBoolean()) {
                return UpdateResult.cancelled();
            }

            String prefix = prefix(origin);
            synchronized (this) {
                if (manifest.version.equals(preferences.getString(prefix + "pending", ""))
                    && validateCached(origin, manifest.version)) {
                    return UpdateResult.unchanged();
                }
            }

            File originRoot = originRoot(origin);
            if (!originRoot.exists() && !originRoot.mkdirs()) {
                throw new IOException("无法创建资源缓存目录");
            }
            staging = new File(originRoot, ".staging-" + UUID.randomUUID());
            if (!staging.mkdirs()) {
                throw new IOException("无法创建资源暂存目录");
            }

            for (ResourceManifest.Entry entry : manifest.files) {
                if (!stillCurrent.getAsBoolean()) {
                    return UpdateResult.cancelled();
                }
                File target = safeFile(staging, entry.relativeFilePath());
                File parent = target.getParentFile();
                if (parent != null && !parent.exists() && !parent.mkdirs()) {
                    throw new IOException("无法创建资源目录");
                }
                downloadAndVerify(origin, entry, target);
            }
            writeFile(new File(staging, "app-resources.json"), manifestBytes);
            boolean fullyVerified = validateDirectory(staging, manifest);
            if (!fullyVerified) {
                throw new IOException("资源完整性复核失败");
            }
            if (!stillCurrent.getAsBoolean()) {
                return UpdateResult.cancelled();
            }

            File finalDirectory = new File(originRoot, manifest.version);
            synchronized (this) {
                if (finalDirectory.exists()) {
                    if (!validateDirectory(finalDirectory, manifest)) {
                        deleteRecursively(finalDirectory);
                        if (!staging.renameTo(finalDirectory)) {
                            throw new IOException("无法替换损坏的资源缓存");
                        }
                    } else {
                        deleteRecursively(staging);
                    }
                } else if (!staging.renameTo(finalDirectory)) {
                    throw new IOException("无法启用完整资源缓存");
                }
                staging = null;
                boolean currentAfterCommit = stillCurrent.getAsBoolean();
                String existingPending = preferences.getString(prefix + "pending", "");
                String nextPending = ResourceStatePolicy.pendingAfterDownload(
                    existingPending,
                    manifest.version,
                    fullyVerified,
                    currentAfterCommit
                );
                if (!currentAfterCommit) {
                    return UpdateResult.cancelled();
                }
                preferences.edit().putString(prefix + "pending", nextPending).apply();
            }
            return UpdateResult.downloaded(manifest.version);
        } catch (Exception error) {
            return UpdateResult.failed(error.getMessage());
        } finally {
            if (staging != null) {
                deleteRecursively(staging);
            }
        }
    }

    private Snapshot loadSnapshot(String origin, String version) throws IOException {
        if (BUILTIN.equals(version)) {
            ResourceManifest manifest = ResourceManifest.parse(
                context.getAssets().open("web/app-resources.json")
            );
            return new Snapshot(origin, BUILTIN, manifest, null, true);
        }
        File directory = new File(originRoot(origin), version);
        ResourceManifest manifest = ResourceManifest.parse(
            new FileInputStream(new File(directory, "app-resources.json"))
        );
        if (!version.equals(manifest.version) || !validateDirectory(directory, manifest)) {
            throw new IOException("缓存资源不完整");
        }
        return new Snapshot(origin, version, manifest, directory, false);
    }

    private boolean validateCached(String origin, String version) {
        try {
            loadSnapshot(origin, version);
            return true;
        } catch (IOException error) {
            return false;
        }
    }

    private boolean validateDirectory(File directory, ResourceManifest manifest) throws IOException {
        for (ResourceManifest.Entry entry : manifest.files) {
            File file = safeFile(directory, entry.relativeFilePath());
            if (!file.isFile() || file.length() != entry.size
                || !entry.sha256.equals(sha256(file))) {
                return false;
            }
        }
        return true;
    }

    private byte[] fetch(String origin, String path, int limit) throws IOException {
        HttpURLConnection connection = open(origin, path);
        try {
            int status = connection.getResponseCode();
            if (status != HttpURLConnection.HTTP_OK) {
                throw new IOException("资源清单请求失败：" + status);
            }
            return ResourceManifest.readLimited(
                new BufferedInputStream(connection.getInputStream()),
                limit
            );
        } finally {
            connection.disconnect();
        }
    }

    private void downloadAndVerify(
        String origin,
        ResourceManifest.Entry entry,
        File target
    ) throws IOException {
        HttpURLConnection connection = open(origin, entry.path);
        try {
            int status = connection.getResponseCode();
            if (status != HttpURLConnection.HTTP_OK) {
                throw new IOException("资源下载失败：" + status);
            }
            long declaredLength = connection.getContentLengthLong();
            if (declaredLength > entry.size || declaredLength > ResourceManifest.MAX_FILE_BYTES) {
                throw new IOException("资源响应大小无效");
            }
            MessageDigest digest = digest();
            long total = 0;
            try (
                InputStream input = new BufferedInputStream(connection.getInputStream());
                BufferedOutputStream output = new BufferedOutputStream(new FileOutputStream(target))
            ) {
                byte[] buffer = new byte[32 * 1024];
                int read;
                while ((read = input.read(buffer)) != -1) {
                    total += read;
                    if (total > entry.size || total > ResourceManifest.MAX_FILE_BYTES) {
                        throw new IOException("资源响应超出声明大小");
                    }
                    digest.update(buffer, 0, read);
                    output.write(buffer, 0, read);
                }
            }
            String actualHash = hex(digest.digest());
            if (total != entry.size || !entry.sha256.equals(actualHash)) {
                throw new IOException("资源大小或摘要校验失败");
            }
        } finally {
            connection.disconnect();
        }
    }

    private HttpURLConnection open(String origin, String path) throws IOException {
        String encodedPath = android.net.Uri.encode(path, "/");
        URL url = new URL(origin + encodedPath);
        if (!ServiceOrigin.matches(origin, android.net.Uri.parse(url.toString()))) {
            throw new IOException("资源地址跨越服务 Origin");
        }
        HttpURLConnection connection = (HttpURLConnection) url.openConnection();
        connection.setInstanceFollowRedirects(false);
        connection.setConnectTimeout(CONNECT_TIMEOUT_MS);
        connection.setReadTimeout(READ_TIMEOUT_MS);
        connection.setRequestProperty("Accept-Encoding", "identity");
        connection.setRequestProperty(ZROK_HEADER, ZROK_VALUE);
        String cookies = CookieManager.getInstance().getCookie(origin);
        if (cookies != null && !cookies.trim().isEmpty()) {
            connection.setRequestProperty("Cookie", cookies);
        }
        return connection;
    }

    private synchronized void cleanup(String origin) {
        String prefix = prefix(origin);
        Set<String> retained = new HashSet<>();
        retained.add(preferences.getString(prefix + "active", BUILTIN));
        retained.add(preferences.getString(prefix + "previous", ""));
        retained.add(preferences.getString(prefix + "pending", ""));
        File[] children = originRoot(origin).listFiles();
        if (children == null) {
            return;
        }
        for (File child : children) {
            if (child.isDirectory() && !child.getName().startsWith(".staging-")
                && !retained.contains(child.getName())) {
                deleteRecursively(child);
            }
        }
    }

    private File originRoot(String origin) {
        return new File(cacheRoot, sha256(origin.getBytes(StandardCharsets.UTF_8)));
    }

    private String prefix(String origin) {
        return sha256(origin.getBytes(StandardCharsets.UTF_8)) + ".";
    }

    private static File safeFile(File root, String relativePath) throws IOException {
        File file = new File(root, relativePath);
        String rootPath = root.getCanonicalPath() + File.separator;
        String filePath = file.getCanonicalPath();
        if (!filePath.startsWith(rootPath)) {
            throw new IOException("资源路径越界");
        }
        return file;
    }

    private static void writeFile(File file, byte[] bytes) throws IOException {
        try (FileOutputStream output = new FileOutputStream(file)) {
            output.write(bytes);
            output.getFD().sync();
        }
    }

    private static String sha256(File file) throws IOException {
        MessageDigest digest = digest();
        try (InputStream input = new BufferedInputStream(new FileInputStream(file))) {
            byte[] buffer = new byte[32 * 1024];
            int read;
            while ((read = input.read(buffer)) != -1) {
                digest.update(buffer, 0, read);
            }
        }
        return hex(digest.digest());
    }

    private static String sha256(byte[] bytes) {
        MessageDigest digest = digest();
        return hex(digest.digest(bytes));
    }

    private static MessageDigest digest() {
        try {
            return MessageDigest.getInstance("SHA-256");
        } catch (NoSuchAlgorithmException impossible) {
            throw new IllegalStateException(impossible);
        }
    }

    private static String hex(byte[] bytes) {
        StringBuilder output = new StringBuilder(bytes.length * 2);
        for (byte value : bytes) {
            output.append(String.format(Locale.ROOT, "%02x", value & 0xff));
        }
        return output.toString();
    }

    private static void deleteRecursively(File target) {
        File[] children = target.listFiles();
        if (children != null) {
            for (File child : children) {
                deleteRecursively(child);
            }
        }
        // Best-effort cleanup only; an incomplete staging directory is never selected.
        target.delete();
    }

    static final class Snapshot {
        final String origin;
        final String version;
        final ResourceManifest manifest;
        private final File directory;
        private final boolean bundled;

        Snapshot(
            String origin,
            String version,
            ResourceManifest manifest,
            File directory,
            boolean bundled
        ) {
            this.origin = origin;
            this.version = version;
            this.manifest = manifest;
            this.directory = directory;
            this.bundled = bundled;
        }

        InputStream open(ResourceManifest.Entry entry, Context context) throws IOException {
            if (bundled) {
                return context.getAssets().open("web/files/" + entry.relativeFilePath());
            }
            return new FileInputStream(safeFile(directory, entry.relativeFilePath()));
        }
    }

    static final class UpdateResult {
        enum Kind {
            UNCHANGED,
            DOWNLOADED,
            UNSUPPORTED,
            CANCELLED,
            FAILED
        }

        final Kind kind;
        final String detail;

        private UpdateResult(Kind kind, String detail) {
            this.kind = kind;
            this.detail = detail;
        }

        static UpdateResult unchanged() {
            return new UpdateResult(Kind.UNCHANGED, "");
        }

        static UpdateResult downloaded(String version) {
            return new UpdateResult(Kind.DOWNLOADED, version);
        }

        static UpdateResult unsupported() {
            return new UpdateResult(Kind.UNSUPPORTED, "");
        }

        static UpdateResult cancelled() {
            return new UpdateResult(Kind.CANCELLED, "");
        }

        static UpdateResult failed(String detail) {
            return new UpdateResult(Kind.FAILED, detail == null ? "未知错误" : detail);
        }
    }
}
