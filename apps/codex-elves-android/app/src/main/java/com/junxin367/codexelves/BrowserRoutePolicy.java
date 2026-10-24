package com.junxin367.codexelves;

import java.net.IDN;
import java.net.InetAddress;
import java.net.URI;
import java.net.URLEncoder;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.Collection;
import java.util.List;
import java.util.Locale;

/** URL-host rules only: ordinary DNS is never resolved to guess a route. */
final class BrowserRoutePolicy {
    static final String PROXY_HOST = "127.0.0.2";
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

    static String normalizeDomain(String input) {
        if (input == null) throw new IllegalArgumentException("域名为空");
        String value = input.trim();
        boolean wildcard = value.startsWith("*.");
        if (wildcard) value = value.substring(2);
        String host = normalizeHost(value);
        if (isIp(host)) throw new IllegalArgumentException("请填写域名，IP 地址按内网规则自动分流");
        return wildcard ? "*." + host : host;
    }

    static boolean isIp(String host) {
        return host.contains(":") || host.matches("[0-9]+(?:\\.[0-9]+){3}");
    }

    static boolean reservedProxyHost(String host) {
        return PROXY_HOST.equals(normalizeHost(host));
    }

    static boolean usesComputer(String input, Collection<String> domains) {
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
        for (String domain : domains)
            if (matchesDomain(host, normalizeDomain(domain))) return true;
        return false;
    }

    private static boolean matchesDomain(String host, String domain) {
        if (domain.startsWith("*.")) {
            return host.endsWith(domain.substring(1));
        }
        return host.equals(domain) || host.endsWith("." + domain);
    }

    static List<String> proxyRules(Collection<String> domains) {
        List<String> rules = new ArrayList<>(List.of(
            "127.0.0.0/8", "10.0.0.0/8", "172.16.0.0/12", "192.168.0.0/16",
            "169.254.0.0/16", "[::1]", "fc00::/7", "fe80::/10", "<local>"));
        for (String suffix : LOCAL_SUFFIXES) {
            rules.add(suffix);
            rules.add("*." + suffix);
            rules.add(suffix + ".");
            rules.add("*." + suffix + ".");
        }
        for (String value : domains) {
            String domain = normalizeDomain(value);
            if (domain.startsWith("*.")) {
                rules.add(domain);
                rules.add(domain + ".");
            } else {
                rules.add(domain);
                rules.add("*." + domain);
                rules.add(domain + ".");
                rules.add("*." + domain + ".");
            }
        }
        return rules;
    }

    static String navigationUrl(String input) {
        if (input == null) throw new IllegalArgumentException("请输入网址或搜索内容");
        String text = input.trim();
        if (text.isEmpty() || text.length() > 8192 || text.matches("(?s).*[\\x00-\\x1f\\x7f].*"))
            throw new IllegalArgumentException("网址或搜索内容无效");
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
                : usesComputer(host, List.of()) || hostPort || port >= 0 ? "http" : "https";
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
}
