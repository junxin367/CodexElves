package com.junxin367.codexelves;

import android.net.Uri;

import java.net.IDN;
import java.net.URI;
import java.net.URISyntaxException;
import java.util.Locale;

final class ServiceOrigin {
    static final String CHANNEL_CONNECT_SOURCES =
        "https://*.ngrok-free.app wss://*.ngrok-free.app https://*.ngrok-free.dev wss://*.ngrok-free.dev "
        + "https://*.ngrok.app wss://*.ngrok.app https://*.ngrok.dev wss://*.ngrok.dev "
        + "https://*.ngrok.io wss://*.ngrok.io https://*.shares.zrok.io wss://*.shares.zrok.io";
    static final String CHANNEL_MEDIA_SOURCES =
        "https://*.ngrok-free.app https://*.ngrok-free.dev https://*.ngrok.app https://*.ngrok.dev "
        + "https://*.ngrok.io https://*.shares.zrok.io";
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
        String scheme = parsed.getScheme().toLowerCase(Locale.ROOT);
        if (!"https".equals(scheme) && !"http".equals(scheme)) {
            throw new IllegalArgumentException("仅支持 HTTP 或 HTTPS 地址");
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
        if (port == ("https".equals(scheme) ? 443 : 80)) {
            port = -1;
        }

        String asciiHost;
        try {
            asciiHost = IDN.toASCII(host, IDN.USE_STD3_ASCII_RULES).toLowerCase(Locale.ROOT);
        } catch (IllegalArgumentException error) {
            throw new IllegalArgumentException("域名格式无效");
        }
        try {
            return new URI(scheme, null, asciiHost, port, null, null, null).toASCIIString();
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

    static String normalizeChannelOrigin(String rawInput) {
        String origin = normalize(rawInput);
        try {
            URI parsed = new URI(origin);
            if (!"https".equals(parsed.getScheme()) || parsed.getPort() != -1
                || !parsed.getHost().matches("[a-zA-Z0-9-]+\\.(shares\\.zrok\\.io|ngrok-free\\.app|ngrok-free\\.dev|ngrok\\.app|ngrok\\.dev|ngrok\\.io)")) {
                throw new IllegalArgumentException("线路域名无效");
            }
            return origin;
        } catch (URISyntaxException error) {
            throw new IllegalArgumentException("线路域名无效");
        }
    }

    /** Permit authenticated session requests and short-lived media links; no documents or assets. */
    static boolean allowsChannelRequest(String rawUrl, String method, java.util.Map<String, String> headers) {
        try {
            URI uri = new URI(rawUrl);
            normalizeChannelOrigin(uri.getScheme() + "://" + uri.getRawAuthority());
            if (uri.getRawFragment() != null) return false;
            boolean preflight = "OPTIONS".equals(method);
            String requestedMethod = preflight ? channelHeader(headers, "Access-Control-Request-Method") : method;
            String path = uri.getRawPath();
            boolean messageSend = "POST".equals(requestedMethod) && path != null
                && path.matches("/api/action/[a-zA-Z]{1,64}");
            boolean media = ("GET".equals(requestedMethod) || "HEAD".equals(requestedMethod)) && path != null
                && path.matches("/api/(images/[a-zA-Z0-9_-]{20,128}(/thumbnail)?|videos/[a-f0-9]{64}(/poster)?)");
            boolean allowed = "GET".equals(requestedMethod)
                && ("/api/session".equals(path) || "/api/channels".equals(path) || "/api/channel-events".equals(path)
                    || path != null && path.matches("/api/receipts/[a-zA-Z0-9-]{1,100}"))
                || "POST".equals(requestedMethod) && path != null
                    && (path.matches("/api/read/[a-zA-Z]+") || path.matches("/api/workspace/changes?")
                        || "/api/logout".equals(path) || "/api/diagnostics".equals(path))
                || messageSend || media;
            if (!allowed) return false;
            if (preflight) {
                boolean authorization = false, idempotency = false;
                for (String name : channelHeader(headers, "Access-Control-Request-Headers").split(",")) {
                    if ("authorization".equalsIgnoreCase(name.trim())) authorization = true;
                    if ("idempotency-key".equalsIgnoreCase(name.trim())) idempotency = true;
                }
                return authorization && (!messageSend || idempotency);
            }
            boolean mediaToken = false;
            if (media && uri.getRawQuery() != null) {
                int count = 0;
                for (String pair : uri.getRawQuery().split("&")) {
                    if (pair.startsWith("channel_token=")) {
                        count++;
                        mediaToken = pair.matches("channel_token=[a-f0-9]{64}");
                    }
                }
                mediaToken = mediaToken && count == 1;
            }
            return (mediaToken || channelHeader(headers, "Authorization").matches("Bearer [a-f0-9]{64}"))
                && (!messageSend || channelHeader(headers, "Idempotency-Key").matches("[a-zA-Z0-9-]{1,100}"));
        } catch (URISyntaxException | IllegalArgumentException error) {
            return false;
        }
    }

    private static String channelHeader(java.util.Map<String, String> headers, String name) {
        for (java.util.Map.Entry<String, String> entry : headers.entrySet())
            if (name.equalsIgnoreCase(entry.getKey())) return entry.getValue() == null ? "" : entry.getValue();
        return "";
    }
}
