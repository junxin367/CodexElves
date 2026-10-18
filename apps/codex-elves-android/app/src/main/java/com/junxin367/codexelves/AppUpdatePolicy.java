package com.junxin367.codexelves;

import org.json.JSONArray;
import org.json.JSONException;
import org.json.JSONObject;

import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.net.URI;
import java.net.URISyntaxException;
import java.nio.charset.StandardCharsets;
import java.security.MessageDigest;
import java.security.NoSuchAlgorithmException;
import java.util.ArrayList;
import java.util.Collections;
import java.util.HashSet;
import java.util.List;
import java.util.Locale;
import java.util.Set;
import java.util.regex.Pattern;

/** The release and APK contract, kept independent of Android UI and PackageManager. */
final class AppUpdatePolicy {
    static final String REPOSITORY = "junxin367/CodexElves";
    static final String APPLICATION_ID = "com.junxin367.codexelves";
    static final String API_ROOT = "https://api.github.com/repos/" + REPOSITORY;
    static final String DOWNLOAD_ROOT = "https://github.com/" + REPOSITORY + "/releases/download/";
    static final int RELEASES_PER_PAGE = 100;
    static final int MAX_RELEASE_PAGES = 5;
    static final int MAX_ANDROID_RELEASES = 32;
    static final int MAX_MANIFEST_BYTES = 64 * 1024;
    static final int MAX_RELEASE_PAGE_BYTES = 4 * 1024 * 1024;
    static final long MAX_APK_BYTES = 256L * 1024L * 1024L;

    private static final Pattern VERSION = Pattern.compile(
        "(0|[1-9][0-9]{0,8})\\.(0|[1-9][0-9]{0,8})\\.(0|[1-9][0-9]{0,8})"
    );
    private static final Pattern SHA256 = Pattern.compile("[a-fA-F0-9]{64}");

    private AppUpdatePolicy() {}

    static String releasesUrl(int page) throws IOException {
        if (page < 1 || page > MAX_RELEASE_PAGES) {
            throw new IOException("更新列表页数无效");
        }
        return API_ROOT + "/releases?per_page=" + RELEASES_PER_PAGE + "&page=" + page;
    }

    static List<Release> parseReleasePage(String json) throws IOException {
        try {
            JSONArray rows = new JSONArray(json);
            if (rows.length() > RELEASES_PER_PAGE) {
                throw new IOException("更新列表超出限制");
            }
            List<Release> result = new ArrayList<>();
            for (int index = 0; index < rows.length(); index++) {
                Object value = rows.get(index);
                if (!(value instanceof JSONObject)) {
                    continue;
                }
                try {
                    Release release = parseRelease((JSONObject) value);
                    if (release != null) {
                        result.add(release);
                    }
                } catch (IOException ignored) {
                    // An invalid release can never become an installation candidate.
                }
            }
            return result;
        } catch (JSONException error) {
            throw new IOException("更新列表格式无效", error);
        }
    }

    static int releasePageLength(String json) throws IOException {
        try {
            return new JSONArray(json).length();
        } catch (JSONException error) {
            throw new IOException("更新列表格式无效", error);
        }
    }

    static List<Release> mostRelevantReleases(List<Release> releases) {
        List<Release> sorted = new ArrayList<>(releases);
        Collections.sort(sorted, (left, right) -> {
            String[] a = left.versionName.split("\\.");
            String[] b = right.versionName.split("\\.");
            for (int index = 0; index < 3; index++) {
                int order = Integer.compare(Integer.parseInt(b[index]), Integer.parseInt(a[index]));
                if (order != 0) {
                    return order;
                }
            }
            return 0;
        });
        return new ArrayList<>(sorted.subList(0, Math.min(MAX_ANDROID_RELEASES, sorted.size())));
    }

    static Release parseRelease(JSONObject row) throws IOException {
        try {
            Object draft = row.get("draft");
            Object prerelease = row.get("prerelease");
            if (!(draft instanceof Boolean) || !(prerelease instanceof Boolean)
                || (Boolean) draft || (Boolean) prerelease) {
                return null;
            }
            String tag = string(row, "tag_name");
            if (!tag.startsWith("android-v")) {
                return null;
            }
            String version = tag.substring("android-v".length());
            if (!VERSION.matcher(version).matches()) {
                return null;
            }
            long id = positiveInteger(row, "id", Long.MAX_VALUE);
            if (!(API_ROOT + "/releases/" + id).equals(string(row, "url"))
                || !("https://github.com/" + REPOSITORY + "/releases/tag/" + tag)
                    .equals(string(row, "html_url"))) {
                throw new IOException("更新不属于指定仓库");
            }
            JSONArray assets = row.getJSONArray("assets");
            if (assets.length() > 200) {
                throw new IOException("更新附件数量超出限制");
            }
            String apkName = "CodexElves-" + version + "-android.apk";
            Asset manifest = null;
            Asset apk = null;
            for (int index = 0; index < assets.length(); index++) {
                JSONObject asset = assets.getJSONObject(index);
                String name = string(asset, "name");
                if (!"android-update.json".equals(name) && !apkName.equals(name)) {
                    continue;
                }
                if (!"uploaded".equals(string(asset, "state"))) {
                    throw new IOException("更新附件尚未上传完成");
                }
                long assetId = positiveInteger(asset, "id", Long.MAX_VALUE);
                if (!(API_ROOT + "/releases/assets/" + assetId).equals(string(asset, "url"))) {
                    throw new IOException("更新附件不属于指定仓库");
                }
                String url = string(asset, "browser_download_url");
                validateAssetUrl(url, tag, name);
                long maximum = apkName.equals(name) ? MAX_APK_BYTES : MAX_MANIFEST_BYTES;
                long size = positiveInteger(asset, "size", maximum);
                Asset parsed = new Asset(name, url, size);
                if (apkName.equals(name)) {
                    if (apk != null) {
                        throw new IOException("安装包附件重复");
                    }
                    apk = parsed;
                } else {
                    if (manifest != null) {
                        throw new IOException("更新清单附件重复");
                    }
                    manifest = parsed;
                }
            }
            return manifest == null || apk == null
                ? null : new Release(tag, version, manifest, apk);
        } catch (JSONException error) {
            throw new IOException("Android Release 格式无效", error);
        }
    }

    static Update parseManifest(
        byte[] bytes,
        Release release,
        long installedVersionCode,
        int deviceSdk
    ) throws IOException {
        if (bytes.length == 0 || bytes.length > MAX_MANIFEST_BYTES
            || bytes.length != release.manifest.size) {
            throw new IOException("更新清单大小无效");
        }
        try {
            JSONObject json = new JSONObject(new String(bytes, StandardCharsets.UTF_8));
            if (positiveInteger(json, "schemaVersion", 1) != 1
                || !APPLICATION_ID.equals(string(json, "applicationId"))
                || !release.versionName.equals(string(json, "versionName"))) {
                throw new IOException("更新清单与 Android Release 不一致");
            }
            long versionCode = positiveInteger(json, "versionCode", 2_100_000_000L);
            int minSdk = (int) positiveInteger(json, "minSdk", Integer.MAX_VALUE);
            if (minSdk < 26 || minSdk > deviceSdk) {
                throw new IOException("新版应用不支持当前 Android 版本");
            }
            JSONObject apk = json.getJSONObject("apk");
            String name = string(apk, "name");
            long size = positiveInteger(apk, "size", MAX_APK_BYTES);
            String sha256 = string(apk, "sha256");
            if (!release.apk.name.equals(name) || release.apk.size != size
                || !SHA256.matcher(sha256).matches()) {
                throw new IOException("更新安装包信息无效");
            }
            if (versionCode <= installedVersionCode) {
                return null;
            }
            return new Update(
                release,
                versionCode,
                minSdk,
                sha256.toLowerCase(Locale.ROOT)
            );
        } catch (JSONException error) {
            throw new IOException("更新清单格式无效", error);
        }
    }

    static void validateAssetUrl(String value, String tag, String fileName) throws IOException {
        URI uri = httpsUri(value);
        String expected = DOWNLOAD_ROOT + tag + "/" + fileName;
        if (!expected.equals(uri.toASCIIString())) {
            throw new IOException("更新附件地址与仓库、标签或文件名不一致");
        }
    }

    static void validateRequestUrl(String value, String initialUrl, boolean api)
        throws IOException {
        URI uri = httpsUri(value);
        if (api) {
            if (!value.equals(initialUrl)
                || !"api.github.com".equals(uri.getHost())
                || !value.startsWith(API_ROOT + "/releases?per_page=")
                || uri.getRawFragment() != null) {
                throw new IOException("更新 API 地址无效");
            }
            boolean knownPage = false;
            for (int page = 1; page <= MAX_RELEASE_PAGES; page++) {
                knownPage |= releasesUrl(page).equals(value);
            }
            if (!knownPage) {
                throw new IOException("更新 API 页数无效");
            }
            return;
        }
        String initialPath = initialUrl.startsWith(DOWNLOAD_ROOT)
            ? initialUrl.substring(DOWNLOAD_ROOT.length()) : "";
        String[] parts = initialPath.split("/", -1);
        if (parts.length != 2 || !parts[0].startsWith("android-v")) {
            throw new IOException("更新下载必须来自指定 Android Release");
        }
        String initialVersion = parts[0].substring("android-v".length());
        if (!VERSION.matcher(initialVersion).matches()
            || !("android-update.json".equals(parts[1])
                || ("CodexElves-" + initialVersion + "-android.apk").equals(parts[1]))) {
            throw new IOException("更新下载附件名无效");
        }
        validateAssetUrl(initialUrl, parts[0], parts[1]);
        if ("github.com".equals(uri.getHost())) {
            if (!value.equals(initialUrl) || !value.startsWith(DOWNLOAD_ROOT)) {
                throw new IOException("已拒绝跨仓库更新跳转");
            }
            return;
        }
        String host = uri.getHost();
        if (!"release-assets.githubusercontent.com".equals(host)
            && !"objects.githubusercontent.com".equals(host)
            && !"github-releases.githubusercontent.com".equals(host)) {
            throw new IOException("已拒绝不受信任的更新下载主机");
        }
        if (uri.getRawPath() == null || uri.getRawPath().length() < 2) {
            throw new IOException("更新下载路径无效");
        }
    }

    private static URI httpsUri(String value) throws IOException {
        try {
            URI uri = new URI(value);
            if (!"https".equals(uri.getScheme())
                || uri.getHost() == null
                || uri.getRawUserInfo() != null
                || uri.getRawFragment() != null
                || uri.getPort() != -1 && uri.getPort() != 443) {
                throw new IOException("更新地址必须使用受信任的 HTTPS");
            }
            return uri;
        } catch (URISyntaxException error) {
            throw new IOException("更新地址格式无效", error);
        }
    }

    static String copy(
        InputStream input,
        OutputStream output,
        long expectedSize,
        long maximumSize,
        AppUpdateCancellation cancellation,
        Progress progress
    ) throws IOException {
        if (expectedSize < 1 || expectedSize > maximumSize) {
            throw new IOException("更新文件大小无效");
        }
        MessageDigest digest = sha256();
        long total = 0;
        byte[] buffer = new byte[32 * 1024];
        int count;
        cancellation.check();
        while ((count = input.read(buffer)) != -1) {
            cancellation.check();
            total += count;
            if (total > expectedSize || total > maximumSize) {
                throw new IOException("更新文件超出声明大小");
            }
            output.write(buffer, 0, count);
            digest.update(buffer, 0, count);
            if (progress != null) {
                progress.onBytes(total, expectedSize);
            }
        }
        cancellation.check();
        if (total != expectedSize) {
            throw new IOException("更新文件下载不完整");
        }
        return hex(digest.digest());
    }

    static void verifySha256(String expected, String actual) throws IOException {
        if (expected == null || !SHA256.matcher(expected).matches()
            || actual == null || !expected.equalsIgnoreCase(actual)) {
            throw new IOException("安装包 SHA-256 校验失败");
        }
    }

    static void validateArchive(
        Update update,
        PackageIdentity installed,
        PackageIdentity archive,
        int deviceSdk
    ) throws IOException {
        if (installed == null || archive == null
            || !APPLICATION_ID.equals(installed.applicationId)
            || !installed.applicationId.equals(archive.applicationId)
            || installed.debuggable || archive.debuggable
            || update.versionCode <= installed.versionCode
            || archive.versionCode != update.versionCode
            || !update.release.versionName.equals(archive.versionName)
            || archive.minSdk != update.minSdk
            || archive.minSdk < 26 || archive.minSdk > deviceSdk) {
            throw new IOException("安装包的包名、版本或 Android 要求不匹配");
        }
        Set<String> currentSigners = certificateDigests(installed.certificates);
        Set<String> updateSigners = certificateDigests(archive.certificates);
        if (currentSigners.isEmpty() || !currentSigners.equals(updateSigners)) {
            throw new IOException("安装包签名与当前应用不一致");
        }
    }

    private static Set<String> certificateDigests(byte[][] certificates) throws IOException {
        Set<String> digests = new HashSet<>();
        if (certificates == null || certificates.length == 0) {
            return digests;
        }
        for (byte[] certificate : certificates) {
            if (certificate == null || certificate.length == 0
                || !digests.add(hex(sha256().digest(certificate)))) {
                throw new IOException("安装包签名信息无效");
            }
        }
        return digests;
    }

    static MessageDigest sha256() {
        try {
            return MessageDigest.getInstance("SHA-256");
        } catch (NoSuchAlgorithmException error) {
            throw new IllegalStateException("SHA-256 unavailable", error);
        }
    }

    static String hex(byte[] bytes) {
        StringBuilder text = new StringBuilder(bytes.length * 2);
        for (byte value : bytes) {
            text.append(Character.forDigit((value >>> 4) & 15, 16));
            text.append(Character.forDigit(value & 15, 16));
        }
        return text.toString();
    }

    private static String string(JSONObject json, String key) throws JSONException, IOException {
        Object value = json.get(key);
        if (!(value instanceof String)) {
            throw new IOException("更新字段类型无效：" + key);
        }
        return (String) value;
    }

    private static long positiveInteger(JSONObject json, String key, long maximum)
        throws JSONException, IOException {
        Object value = json.get(key);
        if (!(value instanceof Integer) && !(value instanceof Long)) {
            throw new IOException("更新字段必须为整数：" + key);
        }
        long number = ((Number) value).longValue();
        if (number < 1 || number > maximum) {
            throw new IOException("更新字段超出限制：" + key);
        }
        return number;
    }

    interface Progress {
        void onBytes(long bytes, long total);
    }

    static final class Asset {
        final String name;
        final String url;
        final long size;

        Asset(String name, String url, long size) {
            this.name = name;
            this.url = url;
            this.size = size;
        }
    }

    static final class Release {
        final String tag;
        final String versionName;
        final Asset manifest;
        final Asset apk;

        Release(String tag, String versionName, Asset manifest, Asset apk) {
            this.tag = tag;
            this.versionName = versionName;
            this.manifest = manifest;
            this.apk = apk;
        }
    }

    static final class Update {
        final Release release;
        final long versionCode;
        final int minSdk;
        final String sha256;

        Update(Release release, long versionCode, int minSdk, String sha256) {
            this.release = release;
            this.versionCode = versionCode;
            this.minSdk = minSdk;
            this.sha256 = sha256;
        }
    }

    static final class PackageIdentity {
        final String applicationId;
        final String versionName;
        final long versionCode;
        final int minSdk;
        final boolean debuggable;
        final byte[][] certificates;

        PackageIdentity(
            String applicationId,
            String versionName,
            long versionCode,
            int minSdk,
            boolean debuggable,
            byte[][] certificates
        ) {
            this.applicationId = applicationId;
            this.versionName = versionName;
            this.versionCode = versionCode;
            this.minSdk = minSdk;
            this.debuggable = debuggable;
            this.certificates = certificates;
        }
    }
}
