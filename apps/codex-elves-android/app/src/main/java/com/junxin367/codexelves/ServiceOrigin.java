package com.junxin367.codexelves;

import android.net.Uri;

import java.net.IDN;
import java.net.URI;
import java.net.URISyntaxException;
import java.util.Locale;

final class ServiceOrigin {
    private ServiceOrigin() {
    }

    static String normalize(String rawInput) {
        String value = rawInput == null ? "" : rawInput.trim();
        if (value.isEmpty()) {
            throw new IllegalArgumentException("请输入服务域名");
        }
        if (!value.contains("://")) {
            value = "https://" + value;
        }

        final URI parsed;
        try {
            parsed = new URI(value);
        } catch (URISyntaxException error) {
            throw new IllegalArgumentException("域名格式无效");
        }
        if (!"https".equalsIgnoreCase(parsed.getScheme())) {
            throw new IllegalArgumentException("仅支持 HTTPS 地址");
        }
        if (parsed.getRawUserInfo() != null) {
            throw new IllegalArgumentException("地址不能包含用户名或密码");
        }
        if (parsed.getRawQuery() != null || parsed.getRawFragment() != null) {
            throw new IllegalArgumentException("地址不能包含查询参数或片段");
        }
        String rawPath = parsed.getRawPath();
        if (rawPath != null && !rawPath.isEmpty() && !"/".equals(rawPath)) {
            throw new IllegalArgumentException("请输入服务根地址，不要包含路径");
        }
        String host = parsed.getHost();
        if (host == null || host.trim().isEmpty()) {
            throw new IllegalArgumentException("域名格式无效");
        }
        String rawAuthority = parsed.getRawAuthority();
        if (rawAuthority == null || rawAuthority.endsWith(":")) {
            throw new IllegalArgumentException("端口无效");
        }
        int port = parsed.getPort();
        if (port == 0 || port < -1 || port > 65535) {
            throw new IllegalArgumentException("端口无效");
        }
        if (port == 443) {
            port = -1;
        }

        String asciiHost;
        try {
            asciiHost = IDN.toASCII(host, IDN.USE_STD3_ASCII_RULES).toLowerCase(Locale.ROOT);
        } catch (IllegalArgumentException error) {
            throw new IllegalArgumentException("域名格式无效");
        }
        try {
            return new URI("https", null, asciiHost, port, null, null, null).toASCIIString();
        } catch (URISyntaxException error) {
            throw new IllegalArgumentException("域名格式无效");
        }
    }

    static boolean matches(String expectedOrigin, Uri uri) {
        if (expectedOrigin == null || uri == null) {
            return false;
        }
        try {
            return expectedOrigin.equals(normalize(uri.getScheme() + "://" + uri.getEncodedAuthority()));
        } catch (IllegalArgumentException error) {
            return false;
        }
    }
}
