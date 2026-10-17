package com.junxin367.codexelves;

import org.json.JSONArray;
import org.json.JSONException;
import org.json.JSONObject;

import java.io.ByteArrayOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.Collections;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.regex.Pattern;

final class ResourceManifest {
    static final int SCHEMA_VERSION = 1;
    static final int MAX_MANIFEST_BYTES = 1024 * 1024;
    static final int MAX_FILES = 512;
    static final long MAX_FILE_BYTES = 12L * 1024L * 1024L;
    static final long MAX_TOTAL_BYTES = 32L * 1024L * 1024L;
    private static final Pattern HASH = Pattern.compile("[0-9a-f]{64}");

    final int minShellVersion;
    final String version;
    final List<Entry> files;
    final Map<String, Entry> byPath;

    private ResourceManifest(int minShellVersion, String version, List<Entry> files) {
        this.minShellVersion = minShellVersion;
        this.version = version;
        this.files = Collections.unmodifiableList(files);
        Map<String, Entry> indexed = new LinkedHashMap<>();
        for (Entry file : files) {
            indexed.put(file.path, file);
        }
        this.byPath = Collections.unmodifiableMap(indexed);
    }

    static ResourceManifest parse(InputStream input) throws IOException {
        return parse(readLimited(input, MAX_MANIFEST_BYTES));
    }

    static ResourceManifest parse(byte[] bytes) throws IOException {
        final JSONObject root;
        try {
            root = new JSONObject(new String(bytes, StandardCharsets.UTF_8));
        } catch (JSONException error) {
            throw new IOException("资源清单不是有效 JSON", error);
        }
        try {
            if (root.getInt("schemaVersion") != SCHEMA_VERSION) {
                throw new IOException("不支持的资源清单版本");
            }
            int minShellVersion = root.getInt("minShellVersion");
            if (minShellVersion < 1) {
                throw new IOException("资源清单的 minShellVersion 无效");
            }
            String version = root.getString("version");
            if (!HASH.matcher(version).matches()) {
                throw new IOException("资源版本摘要无效");
            }

            JSONArray array = root.getJSONArray("files");
            if (array.length() == 0 || array.length() > MAX_FILES) {
                throw new IOException("资源文件数量超出限制");
            }
            List<Entry> files = new ArrayList<>(array.length());
            Map<String, Boolean> duplicates = new LinkedHashMap<>();
            long total = 0;
            String previous = null;
            for (int index = 0; index < array.length(); index++) {
                JSONObject item = array.getJSONObject(index);
                String path = canonicalPath(item.getString("path"));
                if (duplicates.put(path, Boolean.TRUE) != null) {
                    throw new IOException("资源清单包含重复路径");
                }
                if (previous != null && previous.compareTo(path) > 0) {
                    throw new IOException("资源清单未按路径排序");
                }
                previous = path;

                String contentType = item.getString("contentType").trim();
                if (contentType.isEmpty() || contentType.length() > 128
                    || contentType.indexOf('\r') >= 0 || contentType.indexOf('\n') >= 0) {
                    throw new IOException("资源 Content-Type 无效");
                }
                long size = item.getLong("size");
                if (size < 0 || size > MAX_FILE_BYTES) {
                    throw new IOException("单个资源大小超出限制");
                }
                total = Math.addExact(total, size);
                if (total > MAX_TOTAL_BYTES) {
                    throw new IOException("资源总大小超出限制");
                }
                String sha256 = item.getString("sha256").toLowerCase(Locale.ROOT);
                if (!HASH.matcher(sha256).matches()) {
                    throw new IOException("资源摘要无效");
                }
                files.add(new Entry(path, contentType, size, sha256));
            }
            if (!duplicates.containsKey("/")) {
                throw new IOException("资源清单缺少入口页面");
            }
            return new ResourceManifest(minShellVersion, version, files);
        } catch (JSONException | ArithmeticException error) {
            throw new IOException("资源清单字段无效", error);
        }
    }

    static String canonicalPath(String rawPath) throws IOException {
        if (rawPath == null || rawPath.isEmpty() || !rawPath.startsWith("/")
            || rawPath.contains("\\") || rawPath.contains("?") || rawPath.contains("#")
            || rawPath.contains("%")
            || rawPath.indexOf('\0') >= 0) {
            throw new IOException("资源路径无效");
        }
        String decoded = rawPath;
        if (!decoded.startsWith("/") || decoded.contains("\\") || decoded.contains("//")
            || decoded.contains("?") || decoded.contains("#") || decoded.indexOf('\0') >= 0) {
            throw new IOException("资源路径无效");
        }
        String[] segments = decoded.split("/", -1);
        for (String segment : segments) {
            if (".".equals(segment) || "..".equals(segment)) {
                throw new IOException("资源路径包含目录穿越");
            }
        }
        if (decoded.equals("/api") || decoded.startsWith("/api/")
            || decoded.equals("/app-resources.json") || decoded.equals("/native.js")) {
            throw new IOException("资源路径进入保留命名空间");
        }
        return decoded;
    }

    static byte[] readLimited(InputStream input, int limit) throws IOException {
        try (InputStream source = input; ByteArrayOutputStream output = new ByteArrayOutputStream()) {
            byte[] buffer = new byte[16 * 1024];
            int total = 0;
            int read;
            while ((read = source.read(buffer)) != -1) {
                total += read;
                if (total > limit) {
                    throw new IOException("响应大小超出限制");
                }
                output.write(buffer, 0, read);
            }
            return output.toByteArray();
        }
    }

    static final class Entry {
        final String path;
        final String contentType;
        final long size;
        final String sha256;

        Entry(String path, String contentType, long size, String sha256) {
            this.path = path;
            this.contentType = contentType;
            this.size = size;
            this.sha256 = sha256;
        }

        String relativeFilePath() {
            return "/".equals(path) ? "index.html" : path.substring(1);
        }

        boolean sameContent(Entry other) {
            return other != null && path.equals(other.path) && size == other.size
                && contentType.equals(other.contentType) && sha256.equals(other.sha256);
        }
    }
}
