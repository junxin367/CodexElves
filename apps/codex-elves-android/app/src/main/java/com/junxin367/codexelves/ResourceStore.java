package com.junxin367.codexelves;

import android.content.Context;
import android.content.SharedPreferences;
import android.webkit.CookieManager;

import java.io.BufferedInputStream;
import java.io.BufferedOutputStream;
import java.io.ByteArrayOutputStream;
import java.io.File;
import java.io.FileInputStream;
import java.io.FileOutputStream;
import java.io.FilterInputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.InterruptedIOException;
import java.net.HttpURLConnection;
import java.net.URL;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.security.MessageDigest;
import java.security.NoSuchAlgorithmException;
import java.util.ArrayList;
import java.util.HashSet;
import java.util.List;
import java.util.Locale;
import java.util.Set;
import java.util.UUID;
import java.util.function.BooleanSupplier;
import java.util.zip.GZIPInputStream;

final class ResourceStore {
    static final String BUILTIN = "builtin";
    private static final String ZROK_HEADER = "skip_zrok_interstitial";
    private static final String ZROK_VALUE = "1";
    private static final int CONNECT_TIMEOUT_MS = 15_000;
    private static final int READ_TIMEOUT_MS = 30_000;
    private static final long ENCODING_OVERHEAD_BYTES = 64L * 1024L;

    private final Context context;
    private final File cacheRoot;
    private final SharedPreferences preferences;
    private final Set<String> cleanedOrigins = new HashSet<>();

    ResourceStore(Context context) {
        this.context = context.getApplicationContext();
        this.cacheRoot = new File(context.getFilesDir(), "web-resources");
        this.preferences = context.getSharedPreferences("codex_elves_resource_state", Context.MODE_PRIVATE);
    }

    synchronized Snapshot selectForPage(String origin, Cancellation cancellation) throws IOException {
        cancellation.check();
        cleanupAbandonedStaging(origin, cancellation);
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
        Snapshot bundled = null;
        try {
            bundled = loadSnapshot(origin, BUILTIN, cancellation, true);
        } catch (CancelledException cancelled) {
            throw cancelled;
        } catch (IOException invalidBundled) {
            // A damaged APK must not prevent an existing healthy cache from starting.
        }
        if (bundled != null && ResourceStatePolicy.shouldPreferBundled(
            preferences.getString(prefix + "bundledVersion", ""),
            bundled.manifest.version
        )) {
            String previous = active;
            if (BUILTIN.equals(previous) || !validateCached(origin, previous, cancellation)) {
                previous = preferences.getString(prefix + "previous", BUILTIN);
                if (BUILTIN.equals(previous) || !validateCached(origin, previous, cancellation)) {
                    previous = BUILTIN;
                }
            }
            synchronized (cancellation) {
                cancellation.check();
                SharedPreferences.Editor update = preferences.edit()
                    .putString(prefix + "bundledVersion", bundled.manifest.version)
                    .putString(prefix + "active", BUILTIN)
                    .remove(prefix + "pending");
                if (BUILTIN.equals(previous)) {
                    update.remove(prefix + "previous").remove(prefix + "trial");
                } else {
                    update.putString(prefix + "previous", previous)
                        .putString(prefix + "trial", BUILTIN);
                }
                // Record the APK even before ready so rollback cannot repeat this migration.
                update.apply();
            }
            return bundled;
        }
        String pending = preferences.getString(prefix + "pending", "");
        if (!pending.isEmpty()) {
            try {
                Snapshot candidate = loadSnapshot(origin, pending, cancellation, true);
                synchronized (cancellation) {
                    cancellation.check();
                    preferences.edit()
                        .putString(prefix + "previous", active)
                        .putString(prefix + "active", pending)
                        .putString(prefix + "trial", pending)
                        .remove(prefix + "pending")
                        .apply();
                }
                return candidate;
            } catch (CancelledException cancelled) {
                throw cancelled;
            } catch (IOException invalidPending) {
                preferences.edit().remove(prefix + "pending").apply();
            }
        }

        try {
            return loadSnapshot(origin, active, cancellation, true);
        } catch (CancelledException cancelled) {
            throw cancelled;
        } catch (IOException activeError) {
            String previous = preferences.getString(prefix + "previous", BUILTIN);
            Snapshot fallback;
            try {
                fallback = loadSnapshot(origin, previous, cancellation, true);
            } catch (CancelledException cancelled) {
                throw cancelled;
            } catch (IOException previousError) {
                fallback = loadSnapshot(origin, BUILTIN, cancellation, true);
            }
            cancellation.check();
            preferences.edit()
                .putString(prefix + "active", fallback.version)
                .remove(prefix + "trial")
                .remove(prefix + "previous")
                .apply();
            return fallback;
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
        Snapshot current,
        Cancellation cancellation
    ) {
        File staging = null;
        try {
            cancellation.check();
            byte[] manifestBytes = fetchManifest(origin, cancellation);
            ResourceManifest manifest = ResourceManifest.parse(manifestBytes);
            if (manifest.minShellVersion > BuildConfig.SHELL_VERSION) {
                return UpdateResult.unsupported();
            }
            if (current != null && origin.equals(current.origin)
                && manifest.version.equals(current.manifest.version)) {
                return UpdateResult.unchanged();
            }
            cancellation.check();

            String prefix = prefix(origin);
            synchronized (this) {
                if (manifest.version.equals(preferences.getString(prefix + "pending", ""))
                    && validateCached(origin, manifest.version, cancellation)) {
                    return UpdateResult.unchanged();
                }
            }
            List<Snapshot> reusable = reusableSnapshots(origin, current, cancellation);

            File originRoot = originRoot(origin);
            if (!originRoot.exists() && !originRoot.mkdirs()) {
                throw new IOException("无法创建资源缓存目录");
            }
            staging = new File(originRoot, ".staging-" + UUID.randomUUID());
            if (!staging.mkdirs()) {
                throw new IOException("无法创建资源暂存目录");
            }

            for (ResourceManifest.Entry entry : manifest.files) {
                cancellation.check();
                File target = safeFile(staging, entry.relativeFilePath());
                File parent = target.getParentFile();
                if (parent != null && !parent.exists() && !parent.mkdirs()) {
                    throw new IOException("无法创建资源目录");
                }
                if (!reuseAndVerify(reusable, entry, target, cancellation)) {
                    downloadAndVerify(origin, entry, target, cancellation);
                }
            }
            writeFile(new File(staging, "app-resources.json"), manifestBytes);
            boolean fullyVerified = validateDirectory(staging, manifest, cancellation);
            if (!fullyVerified) {
                throw new IOException("资源完整性复核失败");
            }
            cancellation.check();

            File finalDirectory = new File(originRoot, manifest.version);
            synchronized (this) {
                cancellation.check();
                if (finalDirectory.exists()) {
                    if (!validateDirectory(finalDirectory, manifest, cancellation)) {
                        if (manifest.version.equals(preferences.getString(prefix + "active", BUILTIN))
                            || manifest.version.equals(
                                preferences.getString(prefix + "previous", BUILTIN))) {
                            throw new IOException("现用资源目录损坏，保留页面并等待下次启动恢复");
                        }
                        cancellation.check();
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
                synchronized (cancellation) {
                    cancellation.check();
                    String nextPending = ResourceStatePolicy.pendingAfterDownload(
                        preferences.getString(prefix + "pending", ""),
                        manifest.version,
                        fullyVerified,
                        true
                    );
                    preferences.edit().putString(prefix + "pending", nextPending).apply();
                }
            }
            return UpdateResult.downloaded(manifest.version);
        } catch (Exception error) {
            if (cancellation.isCancelled() || error instanceof CancelledException) {
                return UpdateResult.cancelled();
            }
            return UpdateResult.failed(error.getMessage());
        } finally {
            if (staging != null) {
                deleteRecursively(staging);
            }
        }
    }

    private Snapshot loadSnapshot(
        String origin,
        String version,
        Cancellation cancellation,
        boolean verifyFiles
    ) throws IOException {
        cancellation.check();
        if (BUILTIN.equals(version)) {
            ResourceManifest manifest = ResourceManifest.parse(
                context.getAssets().open("web/app-resources.json")
            );
            return new Snapshot(origin, BUILTIN, manifest, null, true);
        }
        if (version == null || !version.matches("[0-9a-f]{64}")) {
            throw new IOException("缓存资源版本无效");
        }
        File directory = new File(originRoot(origin), version);
        ResourceManifest manifest = ResourceManifest.parse(
            new FileInputStream(new File(directory, "app-resources.json"))
        );
        if (!version.equals(manifest.version)
            || verifyFiles && !validateDirectory(directory, manifest, cancellation)) {
            throw new IOException("缓存资源不完整");
        }
        return new Snapshot(origin, version, manifest, directory, false);
    }

    private boolean validateCached(String origin, String version, Cancellation cancellation)
        throws CancelledException {
        try {
            loadSnapshot(origin, version, cancellation, true);
            return true;
        } catch (CancelledException cancelled) {
            throw cancelled;
        } catch (IOException error) {
            return false;
        }
    }

    private boolean validateDirectory(
        File directory,
        ResourceManifest manifest,
        Cancellation cancellation
    ) throws IOException {
        for (ResourceManifest.Entry entry : manifest.files) {
            cancellation.check();
            File file = safeFile(directory, entry.relativeFilePath());
            if (!file.isFile() || file.length() != entry.size
                || !entry.sha256.equals(sha256(file, cancellation))) {
                return false;
            }
        }
        return true;
    }

    private List<Snapshot> reusableSnapshots(
        String origin,
        Snapshot current,
        Cancellation cancellation
    ) throws IOException {
        List<Snapshot> snapshots = new ArrayList<>();
        if (current != null && origin.equals(current.origin)) {
            snapshots.add(current);
        }
        String previous;
        synchronized (this) {
            previous = preferences.getString(prefix(origin) + "previous", BUILTIN);
        }
        if (current == null || !previous.equals(current.version)) {
            try {
                snapshots.add(loadSnapshot(origin, previous, cancellation, false));
            } catch (CancelledException cancelled) {
                throw cancelled;
            } catch (IOException invalidPrevious) {
                // A matching entry is still hashed while copying; a damaged manifest is unusable.
            }
        }
        boolean hasBuiltin = false;
        for (Snapshot candidate : snapshots) {
            hasBuiltin |= candidate.bundled;
        }
        if (!hasBuiltin) {
            snapshots.add(loadSnapshot(origin, BUILTIN, cancellation, false));
        }
        return snapshots;
    }

    private boolean reuseAndVerify(
        List<Snapshot> snapshots,
        ResourceManifest.Entry entry,
        File target,
        Cancellation cancellation
    ) throws IOException {
        for (Snapshot candidate : snapshots) {
            if (!entry.sameContent(candidate.manifest.byPath.get(entry.path))) {
                continue;
            }
            try (InputStream source = candidate.open(entry, context)) {
                copyAndVerify(source, target, entry, cancellation);
                return true;
            } catch (CancelledException cancelled) {
                throw cancelled;
            } catch (IOException unusableFile) {
                if (target.exists() && !target.delete()) {
                    throw new IOException("无法清理损坏的暂存资源", unusableFile);
                }
            }
        }
        return false;
    }

    private byte[] fetchManifest(String origin, Cancellation cancellation) throws IOException {
        HttpURLConnection connection = open(origin, "/app-resources.json", cancellation);
        try {
            int status = connection.getResponseCode();
            if (status != HttpURLConnection.HTTP_OK) {
                throw new IOException("资源清单请求失败：" + status);
            }
            requireContentType(connection, "application/json");
            long limit = ResourceManifest.MAX_MANIFEST_BYTES;
            try (
                EncodedInput encoded = encodedInput(connection, limit, cancellation);
                InputStream decoded = decode(connection, encoded);
                ByteArrayOutputStream output = new ByteArrayOutputStream()
            ) {
                byte[] buffer = new byte[16 * 1024];
                int read;
                while ((read = decoded.read(buffer)) != -1) {
                    cancellation.check();
                    if ((long) output.size() + read > limit) {
                        throw new IOException("资源清单响应大小超出限制");
                    }
                    output.write(buffer, 0, read);
                }
                encoded.verifyComplete();
                return output.toByteArray();
            }
        } finally {
            cancellation.detach(connection);
            connection.disconnect();
        }
    }

    private void downloadAndVerify(
        String origin,
        ResourceManifest.Entry entry,
        File target,
        Cancellation cancellation
    ) throws IOException {
        HttpURLConnection connection = open(origin, entry.path, cancellation);
        try {
            int status = connection.getResponseCode();
            if (status != HttpURLConnection.HTTP_OK) {
                throw new IOException("资源下载失败：" + status);
            }
            requireContentType(connection, entry.contentType);
            try (
                EncodedInput encoded = encodedInput(connection, entry.size, cancellation);
                InputStream decoded = decode(connection, encoded)
            ) {
                copyAndVerify(decoded, target, entry, cancellation);
                encoded.verifyComplete();
            }
        } finally {
            cancellation.detach(connection);
            connection.disconnect();
        }
    }

    private static void copyAndVerify(
        InputStream input,
        File target,
        ResourceManifest.Entry entry,
        Cancellation cancellation
    ) throws IOException {
        MessageDigest digest = digest();
        long total = 0;
        try (
            FileOutputStream fileOutput = new FileOutputStream(target);
            BufferedOutputStream output = new BufferedOutputStream(fileOutput)
        ) {
            byte[] buffer = new byte[32 * 1024];
            int read;
            while ((read = input.read(buffer)) != -1) {
                cancellation.check();
                total += read;
                if (total > entry.size || total > ResourceManifest.MAX_FILE_BYTES) {
                    throw new IOException("资源响应超出声明大小");
                }
                digest.update(buffer, 0, read);
                output.write(buffer, 0, read);
            }
            cancellation.check();
            if (total != entry.size || !entry.sha256.equals(hex(digest.digest()))) {
                throw new IOException("资源大小或摘要校验失败");
            }
            output.flush();
            fileOutput.getFD().sync();
        }
    }

    private static void requireContentType(HttpURLConnection connection, String expected)
        throws IOException {
        String actual = connection.getContentType();
        if (actual == null || !actual.split(";", 2)[0].trim().equalsIgnoreCase(
            expected.split(";", 2)[0].trim())) {
            throw new IOException("资源响应 Content-Type 不匹配");
        }
    }

    private static boolean gzip(HttpURLConnection connection) throws IOException {
        String encoding = connection.getContentEncoding();
        if (encoding == null || encoding.trim().isEmpty()
            || "identity".equalsIgnoreCase(encoding.trim())) {
            return false;
        }
        if (!"gzip".equalsIgnoreCase(encoding.trim())) {
            throw new IOException("不支持的资源 Content-Encoding");
        }
        return true;
    }

    private static EncodedInput encodedInput(
        HttpURLConnection connection,
        long decodedLimit,
        Cancellation cancellation
    ) throws IOException {
        boolean compressed = gzip(connection);
        long limit = decodedLimit + (compressed ? ENCODING_OVERHEAD_BYTES : 0);
        long declaredLength = connection.getContentLengthLong();
        if (declaredLength > limit) {
            throw new IOException("资源编码响应大小超出限制");
        }
        return new EncodedInput(
            new BufferedInputStream(connection.getInputStream()),
            limit,
            declaredLength,
            cancellation
        );
    }

    private static InputStream decode(HttpURLConnection connection, InputStream encoded)
        throws IOException {
        return gzip(connection) ? new GZIPInputStream(encoded, 32 * 1024) : encoded;
    }

    private HttpURLConnection open(String origin, String path, Cancellation cancellation)
        throws IOException {
        cancellation.check();
        String encodedPath = android.net.Uri.encode(path, "/");
        URL url = new URL(origin + encodedPath);
        if (!ServiceOrigin.matches(origin, android.net.Uri.parse(url.toString()))) {
            throw new IOException("资源地址跨越服务 Origin");
        }
        HttpURLConnection connection = (HttpURLConnection) url.openConnection();
        connection.setInstanceFollowRedirects(false);
        connection.setConnectTimeout(CONNECT_TIMEOUT_MS);
        connection.setReadTimeout(READ_TIMEOUT_MS);
        connection.setRequestProperty("Accept-Encoding", "gzip");
        connection.setRequestProperty(ZROK_HEADER, ZROK_VALUE);
        String cookies = CookieManager.getInstance().getCookie(origin);
        if (cookies != null && !cookies.trim().isEmpty()) {
            connection.setRequestProperty("Cookie", cookies);
        }
        cancellation.attach(connection);
        return connection;
    }

    private void cleanupAbandonedStaging(String origin, Cancellation cancellation) throws IOException {
        if (cleanedOrigins.contains(origin)) {
            return;
        }
        File originDirectory = originRoot(origin);
        File[] children = originDirectory.listFiles();
        if (children != null) {
            for (File child : children) {
                cancellation.check();
                if (child.getName().matches(
                    "\\.staging-[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}")) {
                    safeFile(originDirectory, child.getName());
                    deleteRecursively(child);
                }
            }
        }
        cleanedOrigins.add(origin);
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
            if (child.isDirectory() && child.getName().matches("[0-9a-f]{64}")
                && !Files.isSymbolicLink(child.toPath())
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

    private static String sha256(File file, Cancellation cancellation) throws IOException {
        MessageDigest digest = digest();
        try (InputStream input = new BufferedInputStream(new FileInputStream(file))) {
            byte[] buffer = new byte[32 * 1024];
            int read;
            while ((read = input.read(buffer)) != -1) {
                cancellation.check();
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
        if (Files.isSymbolicLink(target.toPath())) {
            target.delete();
            return;
        }
        File[] children = target.listFiles();
        if (children != null) {
            for (File child : children) {
                deleteRecursively(child);
            }
        }
        // Best-effort cleanup only; an incomplete staging directory is never selected.
        target.delete();
    }

    private static final class CancelledException extends InterruptedIOException {
        CancelledException() {
            super("资源操作已取消");
        }
    }

    static final class Cancellation {
        private final BooleanSupplier stillCurrent;
        private volatile boolean cancelled;
        private HttpURLConnection connection;

        Cancellation(BooleanSupplier stillCurrent) {
            this.stillCurrent = stillCurrent;
        }

        boolean isCancelled() {
            return cancelled || Thread.currentThread().isInterrupted() || !stillCurrent.getAsBoolean();
        }

        void check() throws CancelledException {
            if (isCancelled()) {
                throw new CancelledException();
            }
        }

        synchronized void attach(HttpURLConnection next) throws CancelledException {
            if (isCancelled()) {
                next.disconnect();
                check();
            }
            connection = next;
        }

        synchronized void detach(HttpURLConnection previous) {
            if (connection == previous) {
                connection = null;
            }
        }

        void cancel() {
            HttpURLConnection pending;
            synchronized (this) {
                cancelled = true;
                pending = connection;
                connection = null;
            }
            if (pending != null) {
                pending.disconnect();
            }
        }
    }

    private static final class EncodedInput extends FilterInputStream {
        private final long limit;
        private final long declaredLength;
        private final Cancellation cancellation;
        private long count;

        EncodedInput(
            InputStream input,
            long limit,
            long declaredLength,
            Cancellation cancellation
        ) {
            super(input);
            this.limit = limit;
            this.declaredLength = declaredLength;
            this.cancellation = cancellation;
        }

        @Override
        public int read() throws IOException {
            cancellation.check();
            int value = in.read();
            if (value != -1) {
                record(1);
            }
            return value;
        }

        @Override
        public int read(byte[] buffer, int offset, int length) throws IOException {
            cancellation.check();
            int read = in.read(buffer, offset, length);
            if (read > 0) {
                record(read);
            }
            return read;
        }

        private void record(int read) throws IOException {
            count += read;
            if (count > limit || declaredLength >= 0 && count > declaredLength) {
                throw new IOException("资源编码响应大小超出限制");
            }
        }

        void verifyComplete() throws IOException {
            // Gzip may buffer the end of its member. Drain the entity to enforce its wire limit too.
            byte[] buffer = new byte[4096];
            while (read(buffer) != -1) {
                cancellation.check();
            }
            cancellation.check();
            if (declaredLength >= 0 && count != declaredLength) {
                throw new IOException("资源编码响应长度不匹配");
            }
        }
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
