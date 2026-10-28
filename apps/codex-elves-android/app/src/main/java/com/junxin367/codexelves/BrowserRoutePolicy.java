package com.junxin367.codexelves;

import android.net.Uri;
import java.net.IDN;
import java.net.InetAddress;
import java.net.URI;
import java.net.URLEncoder;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.List;
import java.util.Locale;

/** Local-address helpers used for URL normalization and external-link routing. */
final class BrowserRoutePolicy {
    static final String PROXY_HOST = "127.0.0.2";
    static final String PROXY_FALLBACK_HOST = "127.0.0.1";
    private static final List<String> LOCAL_SUFFIXES =
        List.of("localhost", "local", "lan", "home.arpa");

    private BrowserRoutePolicy() { }

    static String normalizeHost(String input) {
        if (input == null) throw new IllegalArgumentException("地址为空");
        String host = input.trim().toLowerCase(Locale.ROOT);
        if (host.startsWith("[") && host.endsWith("]")) host = host.substring(1, host.length() - 1);
        if (host.endsWith(".")) host = host.substring(0, host.length() - 1);
        if (host.isEmpty() || host.length() > 253 || host.matches(".*[\\s/@?#%\\\\].*"))
            throw new IllegalArgumentException("地址格式无效");
        if (host.contains(":")) {
            if (!host.matches("[0-9a-f:.]+")) throw new IllegalArgumentException("IPv6 地址无效");
            try {
                InetAddress address = InetAddress.getByName(host);
                return address.getAddress().length == 4 ? address.getHostAddress() : host;
            }
            catch (Exception invalid) { throw new IllegalArgumentException("IPv6 地址无效"); }
        }
        if (host.matches("[0-9.]+")) {
            String[] parts = host.split("\\.", -1);
            if (parts.length != 4) throw new IllegalArgumentException("请输入完整 IPv4 地址");
            List<String> normalized = new ArrayList<>();
            for (String part : parts) {
                if (part.isEmpty() || part.length() > 3)
                    throw new IllegalArgumentException("IPv4 地址无效");
                int number = Integer.parseInt(part);
                if (number > 255) throw new IllegalArgumentException("IPv4 地址无效");
                normalized.add(Integer.toString(number));
            }
            return String.join(".", normalized);
        }
        String ascii = IDN.toASCII(host, IDN.USE_STD3_ASCII_RULES).toLowerCase(Locale.ROOT);
        if (ascii.isEmpty() || ascii.length() > 253 || ascii.contains(".."))
            throw new IllegalArgumentException("域名格式无效");
        return ascii;
    }

    static boolean isIp(String host) {
        return host.contains(":") || host.matches("[0-9]+(?:\\.[0-9]+){3}");
    }

    static boolean reservedProxyHost(String host) {
        try {
            String normalized = normalizeHost(host);
            return PROXY_HOST.equals(normalized) || PROXY_FALLBACK_HOST.equals(normalized);
        } catch (IllegalArgumentException invalid) {
            return false;
        }
    }

    static boolean usesComputer(String input) {
        String host = normalizeHost(input);
        if (isIp(host)) {
            try {
                byte[] bytes = InetAddress.getByName(host).getAddress();
                if (bytes.length == 4) {
                    int a = bytes[0] & 255, b = bytes[1] & 255;
                    return a == 127 || a == 10 || a == 172 && b >= 16 && b <= 31
                        || a == 192 && b == 168 || a == 169 && b == 254;
                }
                boolean loopback = bytes[15] == 1;
                for (int i = 0; i < 15; i++) loopback &= bytes[i] == 0;
                return loopback || (bytes[0] & 254) == 252
                    || (bytes[0] & 255) == 254 && (bytes[1] & 192) == 128;
            } catch (Exception invalid) { return false; }
        }
        if (!host.contains(".")) return true;
        for (String suffix : LOCAL_SUFFIXES)
            if (matchesDomain(host, suffix)) return true;
        return false;
    }

    private static boolean matchesDomain(String host, String domain) {
        if (domain.startsWith("*.")) {
            return host.endsWith(domain.substring(1));
        }
        return host.equals(domain) || host.endsWith("." + domain);
    }

    static String navigationUrl(String input) {
        if (input == null) throw new IllegalArgumentException("请输入网址或搜索内容");
        String text = input.trim();
        if (text.isEmpty() || text.length() > 8192 || text.matches("(?s).*[\\x00-\\x1f\\x7f].*"))
            throw new IllegalArgumentException("网址或搜索内容无效");
        if (isLocalFileInput(text))
            throw new IllegalArgumentException("本地文件请从远程会话的文件链接打开");
        boolean explicit = text.matches("(?i)^https?://.*");
        boolean hostPort = text.matches("^[^/\\s:]+:[0-9]+(?:[/?#].*)?$");
        if (!explicit && text.matches("^[A-Za-z][A-Za-z0-9+.-]*:.*") && !hostPort)
            throw new IllegalArgumentException("仅支持 HTTP 和 HTTPS 网址");
        if (!explicit && (text.matches("(?s).*\\s.*")
            || !hostPort && !text.contains(".") && !text.startsWith("[")
                && !text.equalsIgnoreCase("localhost"))) {
            try {
                return "https://www.baidu.com/s?wd=" + URLEncoder.encode(text, StandardCharsets.UTF_8.name());
            } catch (java.io.UnsupportedEncodingException impossible) {
                throw new AssertionError(impossible);
            }
        }
        try {
            URI parsed = new URI((explicit ? "" : "https://") + text.replace(" ", "%20"));
            if (parsed.getRawUserInfo() != null || parsed.getRawAuthority() == null)
                throw new IllegalArgumentException("网址不能包含账号密码");
            String authority = parsed.getRawAuthority();
            String rawHost;
            int port = -1;
            if (authority.startsWith("[")) {
                int end = authority.indexOf(']');
                if (end < 0) throw new IllegalArgumentException("IPv6 地址无效");
                rawHost = authority.substring(1, end);
                if (end + 1 < authority.length()) {
                    if (authority.charAt(end + 1) != ':') throw new IllegalArgumentException("端口无效");
                    port = Integer.parseInt(authority.substring(end + 2));
                }
            } else {
                int colon = authority.lastIndexOf(':');
                rawHost = colon < 0 ? authority : authority.substring(0, colon);
                if (colon >= 0) port = Integer.parseInt(authority.substring(colon + 1));
            }
            String host = normalizeHost(rawHost);
            if (port == 0 || port > 65535 || port < -1)
                throw new IllegalArgumentException("端口必须为 1 到 65535");
            if (reservedProxyHost(host)) throw new IllegalArgumentException("该地址由浏览器内部代理使用");
            String scheme = explicit ? parsed.getScheme().toLowerCase(Locale.ROOT)
                : usesComputer(host) || hostPort || port >= 0 ? "http" : "https";
            String targetHost = host.contains(":") ? "[" + host + "]" : host;
            String path = parsed.getRawPath();
            String result = scheme + "://" + targetHost + (port < 0 ? "" : ":" + port)
                + (path == null || path.isEmpty() ? "/" : path)
                + (parsed.getRawQuery() == null ? "" : "?" + parsed.getRawQuery())
                + (parsed.getRawFragment() == null ? "" : "#" + parsed.getRawFragment());
            return new URI(result).toASCIIString();
        } catch (Exception invalid) {
            if (invalid instanceof IllegalArgumentException) throw (IllegalArgumentException) invalid;
            throw new IllegalArgumentException("网址格式无效");
        }
    }

    static boolean isLocalFileInput(String input) {
        if (input == null) return false;
        String text = input.trim();
        return text.matches("(?i)^[a-z]:[\\\\/].*")
            || text.startsWith("\\\\")
            || text.matches("(?i)^file:(//)?[^\\s].*");
    }

    static String workspaceViewThread(String rawUrl) {
        String[] parts = workspaceViewParts(rawUrl);
        return parts == null ? "" : parts[0];
    }

    static String workspaceViewFile(String rawUrl) {
        String[] parts = workspaceViewParts(rawUrl);
        return parts == null ? "" : parts[1];
    }

    static boolean isWorkspaceViewUrl(String rawUrl) {
        return workspaceViewParts(rawUrl) != null;
    }

    static String workspaceViewDisplay(String rawUrl) {
        String file = workspaceViewFile(rawUrl);
        return file.isEmpty() ? "" : file;
    }

    static String workspaceViewUrl(String origin, String threadId, String input) {
        if (origin == null || threadId == null || threadId.isEmpty() || input == null)
            throw new IllegalArgumentException("本地文件地址无效");
        String path = input.trim().replace('\\', '/');
        if (path.matches("(?i)^file:(//)?[^\\s].*")) {
            Uri uri = Uri.parse(path);
            if (uri.getHost() != null && !"localhost".equalsIgnoreCase(uri.getHost()))
                throw new IllegalArgumentException("本地文件地址无效");
            path = uri.getPath() == null ? "" : uri.getPath();
        }
        if (path.startsWith("/") && path.length() > 2 && path.charAt(2) == ':')
            path = path.substring(1);
        String[] segments = path.split("/");
        if (segments.length == 0 || path.isEmpty()) throw new IllegalArgumentException("本地文件地址无效");
        StringBuilder encoded = new StringBuilder(origin)
            .append("/api/workspace/view/").append(encodeSegment(threadId));
        boolean hasSegment = false;
        for (String segment : segments) {
            if (segment.isEmpty() || ".".equals(segment) || "..".equals(segment))
                throw new IllegalArgumentException("本地文件地址无效");
            encoded.append('/').append(encodeSegment(segment));
            hasSegment = true;
        }
        if (!hasSegment) throw new IllegalArgumentException("本地文件地址无效");
        return encoded.toString();
    }

    private static String[] workspaceViewParts(String rawUrl) {
        if (rawUrl == null) return null;
        Uri uri = Uri.parse(rawUrl);
        if (!"http".equalsIgnoreCase(uri.getScheme()) && !"https".equalsIgnoreCase(uri.getScheme()))
            return null;
        if (uri.getQuery() != null || uri.getFragment() != null) return null;
        String path = uri.getPath();
        String prefix = "/api/workspace/view/";
        if (path == null || !path.startsWith(prefix)) return null;
        String rest = path.substring(prefix.length());
        int separator = rest.indexOf('/');
        if (separator <= 0 || separator == rest.length() - 1) return null;
        String threadId = rest.substring(0, separator);
        String file = rest.substring(separator + 1);
        if (!threadId.matches("[A-Za-z0-9_-]{1,120}")
            || file.isEmpty() || file.contains("\\")
            || java.util.Arrays.stream(file.split("/"))
                .anyMatch(part -> part.isEmpty() || ".".equals(part) || "..".equals(part))) return null;
        return new String[]{threadId, file};
    }

    private static String encodeSegment(String value) {
        try {
            return URLEncoder.encode(value, StandardCharsets.UTF_8.name())
                .replace("+", "%20");
        } catch (java.io.UnsupportedEncodingException impossible) {
            throw new AssertionError(impossible);
        }
    }
}
