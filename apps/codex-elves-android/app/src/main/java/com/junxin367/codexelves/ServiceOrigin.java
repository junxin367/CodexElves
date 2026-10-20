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
        + "https://*.ngrok.io wss://*.ngrok.io";
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

    /** Permit credentialed secondary reads and idempotent message sends; no documents or assets. */
    static boolean allowsChannelRequest(String rawUrl, String method, java.util.Map<String, String> headers) {
        try {
            URI uri = new URI(rawUrl);
            String origin = normalize(uri.getScheme() + "://" + uri.getRawAuthority());
            String host = new URI(origin).getHost();
            if (uri.getRawFragment() != null || uri.getPort() != -1 && uri.getPort() != 443
                || !host.matches("[a-zA-Z0-9-]+\\.(ngrok-free\\.app|ngrok-free\\.dev|ngrok\\.app|ngrok\\.dev|ngrok\\.io)"))
                return false;
            boolean preflight = "OPTIONS".equals(method);
            String requestedMethod = preflight ? channelHeader(headers, "Access-Control-Request-Method") : method;
            String path = uri.getRawPath();
            boolean messageSend = "POST".equals(requestedMethod)
                && ("/api/action/send".equals(path) || "/api/action/create".equals(path) || "/api/action/setGoal".equals(path));
            boolean allowed = "GET".equals(requestedMethod)
                && ("/api/session".equals(path) || "/api/channels".equals(path) || "/api/channel-events".equals(path)
                    || path != null && path.matches("/api/receipts/[a-zA-Z0-9-]{1,100}"))
                || "POST".equals(requestedMethod) && path != null && path.matches("/api/read/[a-zA-Z]+")
                || messageSend;
            if (!allowed) return false;
            if (preflight) {
                boolean authorization = false, idempotency = false;
                for (String name : channelHeader(headers, "Access-Control-Request-Headers").split(",")) {
                    if ("authorization".equalsIgnoreCase(name.trim())) authorization = true;
                    if ("idempotency-key".equalsIgnoreCase(name.trim())) idempotency = true;
                }
                return authorization && (!messageSend || idempotency);
            }
            return channelHeader(headers, "Authorization").matches("Bearer [a-f0-9]{64}")
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
