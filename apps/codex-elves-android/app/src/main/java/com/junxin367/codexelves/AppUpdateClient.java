package com.junxin367.codexelves;

import android.content.Context;
import android.content.pm.ApplicationInfo;
import android.content.pm.PackageInfo;
import android.content.pm.PackageManager;
import android.content.pm.Signature;
import android.os.Build;

import java.io.BufferedInputStream;
import java.io.BufferedOutputStream;
import java.io.ByteArrayOutputStream;
import java.io.File;
import java.io.FileInputStream;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.net.HttpURLConnection;
import java.net.URL;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.HashSet;
import java.util.List;
import java.util.Set;
import java.util.concurrent.TimeUnit;

import javax.net.ssl.HttpsURLConnection;

/** Public GitHub requests deliberately do not use the remote-service HTTP or cookie helpers. */
final class AppUpdateClient {
    private static final int MAX_REDIRECTS = 5;
    private final Context context;

    AppUpdateClient(Context context) {
        this.context = context.getApplicationContext();
    }

    AppUpdatePolicy.Update check(AppUpdateCancellation cancellation) throws IOException {
        requireReleaseBuild();
        cleanExpiredFiles();
        List<AppUpdatePolicy.Release> releases = new ArrayList<>();
        Set<String> tags = new HashSet<>();
        for (int page = 1; page <= AppUpdatePolicy.MAX_RELEASE_PAGES; page++) {
            cancellation.check();
            String url = AppUpdatePolicy.releasesUrl(page);
            byte[] bytes = readBytes(
                url, true, -1, AppUpdatePolicy.MAX_RELEASE_PAGE_BYTES, cancellation
            );
            String json = new String(bytes, StandardCharsets.UTF_8);
            for (AppUpdatePolicy.Release release : AppUpdatePolicy.parseReleasePage(json)) {
                if (tags.add(release.tag)) {
                    releases.add(release);
                }
            }
            if (AppUpdatePolicy.releasePageLength(json) < AppUpdatePolicy.RELEASES_PER_PAGE) {
                break;
            }
        }

        AppUpdatePolicy.Update selected = null;
        IOException manifestFailure = null;
        for (AppUpdatePolicy.Release release : AppUpdatePolicy.mostRelevantReleases(releases)) {
            cancellation.check();
            try {
                byte[] manifest = readBytes(
                    release.manifest.url,
                    false,
                    release.manifest.size,
                    AppUpdatePolicy.MAX_MANIFEST_BYTES,
                    cancellation
                );
                AppUpdatePolicy.Update update = AppUpdatePolicy.parseManifest(
                    manifest, release, BuildConfig.VERSION_CODE, Build.VERSION.SDK_INT
                );
                if (update != null && (selected == null
                    || update.versionCode > selected.versionCode)) {
                    selected = update;
                }
            } catch (IOException error) {
                cancellation.check();
                manifestFailure = error;
            }
        }
        cancellation.check();
        if (selected == null && manifestFailure != null) {
            throw manifestFailure;
        }
        return selected;
    }

    Downloaded download(
        AppUpdatePolicy.Update update,
        AppUpdateCancellation cancellation,
        AppUpdatePolicy.Progress progress
    ) throws IOException {
        requireReleaseBuild();
        AppUpdatePolicy.validateAssetUrl(
            update.release.apk.url, update.release.tag, update.release.apk.name
        );
        File directory = updatesDirectory();
        cleanExpiredFiles();
        File partial = File.createTempFile("android-", ".part", directory);
        File apk = new File(directory, partial.getName().replace(".part", ".apk"));
        boolean accepted = false;
        try {
            HttpURLConnection connection = open(update.release.apk.url, false, cancellation);
            String digest;
            try {
                validateContentLength(connection, update.release.apk.size, AppUpdatePolicy.MAX_APK_BYTES);
                try (
                    InputStream input = new BufferedInputStream(connection.getInputStream());
                    OutputStream output = new BufferedOutputStream(new FileOutputStream(partial))
                ) {
                    digest = AppUpdatePolicy.copy(
                        input,
                        output,
                        update.release.apk.size,
                        AppUpdatePolicy.MAX_APK_BYTES,
                        cancellation,
                        progress
                    );
                }
            } finally {
                close(connection, cancellation);
            }
            AppUpdatePolicy.verifySha256(update.sha256, digest);
            cancellation.check();
            if (!partial.renameTo(apk)) {
                throw new IOException("无法保存更新安装包");
            }
            verifyPackage(apk, update, cancellation);
            cancellation.check();
            accepted = true;
            return new Downloaded(apk, update);
        } finally {
            partial.delete();
            if (!accepted) {
                apk.delete();
            }
        }
    }

    void verifyDownloaded(Downloaded downloaded, AppUpdateCancellation cancellation)
        throws IOException {
        requireReleaseBuild();
        File file = downloaded.file;
        File directory = updatesDirectory();
        if (!directory.getCanonicalFile().equals(file.getCanonicalFile().getParentFile())
            || !file.getName().startsWith("android-") || !file.getName().endsWith(".apk")
            || !file.isFile() || file.length() != downloaded.update.release.apk.size) {
            throw new IOException("更新安装包已失效，请重新下载");
        }
        String digest;
        try (InputStream input = new BufferedInputStream(new FileInputStream(file))) {
            digest = AppUpdatePolicy.copy(
                input,
                new OutputStream() {
                    @Override
                    public void write(int value) {}

                    @Override
                    public void write(byte[] bytes, int offset, int length) {}
                },
                downloaded.update.release.apk.size,
                AppUpdatePolicy.MAX_APK_BYTES,
                cancellation,
                null
            );
        }
        AppUpdatePolicy.verifySha256(downloaded.update.sha256, digest);
        verifyPackage(file, downloaded.update, cancellation);
    }

    @SuppressWarnings("deprecation")
    private void verifyPackage(
        File file,
        AppUpdatePolicy.Update update,
        AppUpdateCancellation cancellation
    ) throws IOException {
        cancellation.check();
        PackageManager manager = context.getPackageManager();
        int flags = Build.VERSION.SDK_INT >= 28
            ? PackageManager.GET_SIGNING_CERTIFICATES : PackageManager.GET_SIGNATURES;
        try {
            PackageInfo installed = manager.getPackageInfo(context.getPackageName(), flags);
            PackageInfo archive = manager.getPackageArchiveInfo(file.getAbsolutePath(), flags);
            AppUpdatePolicy.validateArchive(
                update, identity(installed), identity(archive), Build.VERSION.SDK_INT
            );
            cancellation.check();
        } catch (PackageManager.NameNotFoundException | RuntimeException error) {
            throw new IOException("无法校验更新安装包", error);
        }
    }

    @SuppressWarnings("deprecation")
    private static AppUpdatePolicy.PackageIdentity identity(PackageInfo info) {
        if (info == null || info.applicationInfo == null) {
            return null;
        }
        Signature[] signatures;
        if (Build.VERSION.SDK_INT >= 28) {
            signatures = info.signingInfo == null
                ? null : info.signingInfo.getApkContentsSigners();
        } else {
            signatures = info.signatures;
        }
        byte[][] certificates = signatures == null ? null : new byte[signatures.length][];
        if (signatures != null) {
            for (int index = 0; index < signatures.length; index++) {
                certificates[index] = signatures[index].toByteArray();
            }
        }
        return new AppUpdatePolicy.PackageIdentity(
            info.packageName,
            info.versionName,
            Build.VERSION.SDK_INT >= 28 ? info.getLongVersionCode() : info.versionCode,
            info.applicationInfo.minSdkVersion,
            (info.applicationInfo.flags & ApplicationInfo.FLAG_DEBUGGABLE) != 0,
            certificates
        );
    }

    private byte[] readBytes(
        String url,
        boolean api,
        long expectedSize,
        int maximumSize,
        AppUpdateCancellation cancellation
    ) throws IOException {
        HttpURLConnection connection = open(url, api, cancellation);
        try {
            validateContentLength(connection, expectedSize, maximumSize);
            ByteArrayOutputStream output = new ByteArrayOutputStream();
            try (InputStream input = new BufferedInputStream(connection.getInputStream())) {
                if (expectedSize > 0) {
                    AppUpdatePolicy.copy(
                        input, output, expectedSize, maximumSize, cancellation, null
                    );
                } else {
                    byte[] buffer = new byte[16 * 1024];
                    int count;
                    int total = 0;
                    while ((count = input.read(buffer)) != -1) {
                        cancellation.check();
                        total += count;
                        if (total > maximumSize) {
                            throw new IOException("更新列表大小超出限制");
                        }
                        output.write(buffer, 0, count);
                    }
                }
            }
            cancellation.check();
            return output.toByteArray();
        } finally {
            close(connection, cancellation);
        }
    }

    private static HttpURLConnection open(
        String initialUrl,
        boolean api,
        AppUpdateCancellation cancellation
    ) throws IOException {
        String current = initialUrl;
        for (int hop = 0; hop <= MAX_REDIRECTS; hop++) {
            cancellation.check();
            AppUpdatePolicy.validateRequestUrl(current, initialUrl, api);
            HttpsURLConnection connection = (HttpsURLConnection) new URL(current).openConnection();
            boolean keepOpen = false;
            try {
                cancellation.attach(connection);
                connection.setInstanceFollowRedirects(false);
                connection.setConnectTimeout(cancellation.timeout(15_000));
                connection.setReadTimeout(cancellation.timeout(20_000));
                connection.setUseCaches(false);
                connection.setRequestMethod("GET");
                connection.setRequestProperty("Accept-Encoding", "identity");
                connection.setRequestProperty(
                    "Accept", api ? "application/vnd.github+json" : "application/octet-stream"
                );
                connection.setRequestProperty(
                    "User-Agent", "CodexElvesAndroid/" + BuildConfig.VERSION_NAME
                );
                // Never read CookieManager, SessionStore or a remote-service access key here.
                connection.setRequestProperty("Cookie", "");
                connection.setRequestProperty("Authorization", "");
                if (api) {
                    connection.setRequestProperty("X-GitHub-Api-Version", "2022-11-28");
                }
                int status = connection.getResponseCode();
                cancellation.check();
                if (status == 301 || status == 302 || status == 303
                    || status == 307 || status == 308) {
                    String location = connection.getHeaderField("Location");
                    if (api || location == null || hop == MAX_REDIRECTS) {
                        throw new IOException("更新下载跳转无效");
                    }
                    current = new URL(new URL(current), location).toExternalForm();
                    continue;
                }
                if (status != HttpURLConnection.HTTP_OK) {
                    throw new IOException("GitHub 更新服务返回 " + status);
                }
                String encoding = connection.getContentEncoding();
                if (encoding != null && !"identity".equalsIgnoreCase(encoding)) {
                    throw new IOException("更新文件使用了不支持的传输编码");
                }
                keepOpen = true;
                return connection;
            } finally {
                if (!keepOpen) {
                    close(connection, cancellation);
                }
            }
        }
        throw new IOException("更新下载跳转次数超出限制");
    }

    private static void validateContentLength(
        HttpURLConnection connection,
        long expectedSize,
        long maximumSize
    ) throws IOException {
        long length = connection.getContentLengthLong();
        if (length > maximumSize || expectedSize > maximumSize
            || expectedSize > 0 && length >= 0 && length != expectedSize) {
            throw new IOException("更新文件的实际大小与清单不一致");
        }
    }

    private static void close(HttpURLConnection connection, AppUpdateCancellation cancellation) {
        cancellation.detach(connection);
        connection.disconnect();
    }

    private File updatesDirectory() throws IOException {
        File directory = new File(context.getCacheDir(), "updates");
        if (!directory.isDirectory() && !directory.mkdirs()) {
            throw new IOException("无法创建更新缓存目录");
        }
        return directory;
    }

    private void cleanExpiredFiles() throws IOException {
        File directory = updatesDirectory();
        File[] files = directory.listFiles();
        if (files == null) {
            return;
        }
        long oldest = System.currentTimeMillis() - TimeUnit.DAYS.toMillis(1);
        for (File file : files) {
            if (file.isFile() && file.getName().startsWith("android-")
                && (file.getName().endsWith(".part") || file.getName().endsWith(".apk"))
                && file.lastModified() < oldest) {
                file.delete();
            }
        }
    }

    private void requireReleaseBuild() throws IOException {
        if (BuildConfig.DEBUG || !AppUpdatePolicy.APPLICATION_ID.equals(context.getPackageName())) {
            throw new IOException("开发版本不支持安装正式更新");
        }
    }

    static final class Downloaded {
        final File file;
        final AppUpdatePolicy.Update update;

        Downloaded(File file, AppUpdatePolicy.Update update) {
            this.file = file;
            this.update = update;
        }

        void delete() {
            file.delete();
        }
    }
}
