package com.junxin367.codexelves;

import android.net.Uri;

import org.json.JSONException;
import org.json.JSONObject;

import java.util.Arrays;
import java.util.HashSet;
import java.util.Iterator;
import java.util.Locale;
import java.util.Set;

/** A separate, deliberately small contract for data that may leave the device. */
final class DiagnosticsPolicy {
    static final int MAX_EVENT_CHARS = 2048;
    private static final Set<String> STATIC_PATHS = set(
        "/", "/index.html", "/app-resources.json", "/app.js", "/diagnostics.js",
        "/session-tools.js", "/session-menu.js", "/attachments.js", "/events-client.js",
        "/events-worker.js", "/channels.js", "/markdown.js", "/markdown-renderer.js",
        "/vendor/marked.esm.js", "/vendor/dompurify.esm.js", "/vendor/mermaid.esm.js",
        "/message-actions.js", "/platform.js", "/pwa.js", "/recovery-store.js", "/sw.js",
        "/style.css", "/manifest.webmanifest", "/icons/icon-192.png", "/icons/icon-512.png",
        "/icons/apple-touch-icon.png", "/api/connect", "/api/session", "/api/channels",
        "/api/channel-events", "/api/events", "/api/logout", "/api/diagnostics",
        "/api/workspace/change", "/api/workspace/changes", "/api/receipts", "/other"
    );
    private static final Set<String> REASONS = set(
        "unknown", "startup-timeout", "load-failed", "ssl-failed", "renderer-crashed",
        "renderer-killed", "local-read-failed", "select-failed", "auth-required", "logged-out",
        "http-error", "network-error", "invalid-response", "invalid-content-type",
        "unsupported", "unchanged", "downloaded", "cancelled", "failed", "ready",
        "bridge-unavailable", "export-failed", "snapshot-failed", "login-failed",
        "login-success", "cookie-failed", "origin-changed", "error", "unhandled-rejection",
        "manual", "remote", "hidden", "visible", "event", "request", "response", "api",
        "image", "reload", "offline", "online", "reconnect", "auth-lost", "success",
        "timeout", "response-read-failed", "visibility", "pagehide", "upload-failed",
        "native-failed", "cors-blocked", "corp-blocked", "net-failed", "console-error"
    );
    private static final Set<String> ERROR_TYPES = set(
        "unknown", "network", "timeout", "http", "ssl", "renderer", "resource", "parse",
        "abort", "Error", "TypeError", "RangeError", "ReferenceError", "SyntaxError",
        "URIError", "EvalError", "DOMException", "AbortError", "TimeoutError",
        "NetworkError", "SecurityError", "IOException", "InterruptedIOException",
        "FileNotFoundException", "SSLException", "SSLHandshakeException",
        "SocketTimeoutException", "ConnectException", "UnknownHostException",
        "JSONException", "IllegalArgumentException", "IllegalStateException",
        "SecurityException", "RuntimeException", "CancelledException"
    );

    private DiagnosticsPolicy() {}

    static boolean isRequestType(String type) {
        return "diagnostics-log".equals(type) || "diagnostics-snapshot".equals(type)
            || "diagnostics-export".equals(type);
    }

    static boolean isValidRequest(JSONObject payload) {
        String type = payload.optString("type", "");
        if (!isRequestType(type) || payload.length() != 2) {
            return false;
        }
        if (!"diagnostics-log".equals(type)) {
            return payload.opt("requestId") instanceof String
                && validRequestId(payload.optString("requestId"));
        }
        Object rawEvent = payload.opt("event");
        if (!(rawEvent instanceof JSONObject)) {
            return false;
        }
        JSONObject event = (JSONObject) rawEvent;
        return event.length() == 2 && event.opt("name") instanceof String
            && validName(event.optString("name")) && event.opt("fields") instanceof JSONObject
            && validFields(event.optJSONObject("fields"))
            && event.toString().length() <= MAX_EVENT_CHARS;
    }

    static boolean validRequestId(String value) {
        return value != null && value.matches("[A-Za-z0-9_-]{1,80}");
    }

    static boolean validName(String value) {
        return value != null && value.length() <= 64
            && value.matches("[a-z][a-z0-9]*(?:[.-][a-z0-9]+)*");
    }

    static boolean validFields(JSONObject fields) {
        if (fields == null || fields.length() > 33) {
            return false;
        }
        Iterator<String> keys = fields.keys();
        while (keys.hasNext()) {
            String key = keys.next();
            if (!validField(key, fields.opt(key))) {
                return false;
            }
        }
        return true;
    }

    static JSONObject fields(Object... pairs) {
        JSONObject result = new JSONObject();
        for (int index = 0; index + 1 < pairs.length; index += 2) {
            String key = (String) pairs[index];
            Object value = pairs[index + 1];
            if (validField(key, value)) {
                try {
                    result.put(key, value);
                } catch (JSONException ignored) {
                    // Values pass the finite-number/type checks before they reach JSON.
                }
            }
        }
        return result;
    }

    private static boolean validField(String key, Object value) {
        if ("errorCode".equals(key)) {
            if (!(value instanceof Number)) return false;
            double code = ((Number) value).doubleValue();
            return Double.isFinite(code) && code == Math.floor(code)
                && code >= -1000 && code <= 1000;
        }
        if ("visible".equals(key) || "online".equals(key)) {
            return value instanceof Boolean;
        }
        if (set("status", "durationMs", "bytes", "sdk", "generation", "expectedBytes",
            "loadedBytes", "line", "width", "height", "count", "port").contains(key)) {
            if (!(value instanceof Number)) {
                return false;
            }
            double number = ((Number) value).doubleValue();
            if (!Double.isFinite(number) || number < 0 || number != Math.floor(number)) {
                return false;
            }
            if ("status".equals(key)) return number == 0 || number >= 100 && number <= 599;
            if ("sdk".equals(key)) return number >= 1 && number <= 10_000;
            if ("durationMs".equals(key)) return number <= 86_400_000;
            if ("line".equals(key) || "width".equals(key) || "height".equals(key)) {
                return number <= 10_000_000;
            }
            if ("port".equals(key)) return number >= 1 && number <= 65_535;
            if ("count".equals(key)) return number <= 10_000;
            return number <= 9_007_199_254_740_991d;
        }
        if (!(value instanceof String) || ((String) value).length() > 160) {
            return false;
        }
        String text = (String) value;
        switch (key) {
            case "host":
                return text.matches("[a-z0-9](?:[a-z0-9.-]{0,158}[a-z0-9])?");
            case "path":
                return isSafePath(text);
            case "method":
                return set("GET", "POST", "HEAD", "OPTIONS", "PUT", "PATCH", "DELETE").contains(text);
            case "mime":
                return set("text/html", "text/plain", "text/css", "text/javascript", "text/event-stream",
                    "application/javascript", "application/json", "application/manifest+json",
                    "application/octet-stream", "image/png", "image/jpeg", "image/webp",
                    "image/gif", "image/avif", "image/svg+xml", "video/mp4", "video/webm", "video/ogg",
                    "audio/mpeg", "audio/mp4", "audio/webm", "unknown", "other").contains(text);
            case "errorType":
                return ERROR_TYPES.contains(text);
            case "reason":
                return REASONS.contains(text);
            case "phase":
                return set("create", "start", "resume", "pause", "stop", "destroy", "login",
                    "page", "ready", "authenticated", "check", "download", "select", "rollback",
                    "request", "response", "end", "failure", "success", "pending", "failed",
                    "complete", "before", "after", "body").contains(text);
            case "version":
                return text.matches("[0-9]+(?:\\.[0-9]+){1,3}(?:-debug)?")
                    || text.matches("[a-f0-9]{64}") || "builtin".equals(text);
            case "resourceVersion":
                return "builtin".equals(text) || text.matches("[a-f0-9]{64}");
            case "webViewVersion":
                return "unknown".equals(text) || text.matches("[0-9]+(?:\\.[0-9]+){1,4}");
            case "routeName":
                return text.matches("[a-z][a-z0-9.-]{0,63}");
            case "readyState":
                return set("loading", "interactive", "complete").contains(text);
            case "imageKind":
                return set("thumbnail", "full", "preview", "poster", "image", "video",
                    "unknown", "attachment").contains(text);
            case "requestId":
                return validRequestId(text);
            case "network":
                return set("wifi", "cellular", "ethernet", "vpn", "other", "none",
                    "unknown").contains(text);
            default:
                return false;
        }
    }

    static JSONObject urlFields(String rawUrl, Object... extraFields) {
        JSONObject result = fields(extraFields);
        if (rawUrl == null || rawUrl.length() > 16_384) {
            return result;
        }
        try {
            Uri uri = Uri.parse(rawUrl);
            String scheme = uri.getScheme();
            if (scheme == null || !("http".equalsIgnoreCase(scheme)
                || "https".equalsIgnoreCase(scheme))) {
                return result;
            }
            String host = uri.getHost();
            String safeHost = host == null ? "" : host.toLowerCase(Locale.ROOT);
            if (validField("host", safeHost)) result.put("host", safeHost);
            result.put("path", safePath(uri.getEncodedPath()));
        } catch (Exception ignored) {
            return result;
        }
        return result;
    }

    static String safePath(String path) {
        return isSafePath(path) ? path : "<other>";
    }

    private static boolean isSafePath(String path) {
        return path != null && path.length() <= 160
            && ("<other>".equals(path) || STATIC_PATHS.contains(path)
                || path.matches("/api/(action|read)/[A-Za-z]{1,64}")
                || path.matches("/api/images/[A-Za-z0-9_-]{20,128}(?:/thumbnail)?")
                || path.matches("/api/videos/[a-f0-9]{64}(?:/poster)?"));
    }

    static String mime(String rawMime) {
        if (rawMime == null) return "unknown";
        String normalized = rawMime.split(";", 2)[0].trim().toLowerCase(Locale.ROOT);
        return validField("mime", normalized) ? normalized : "other";
    }

    static String errorType(Throwable error) {
        String name = error == null ? "unknown" : error.getClass().getSimpleName();
        return ERROR_TYPES.contains(name) ? name : "unknown";
    }

    static String consoleReason(String message) {
        if (message == null || message.length() > 16_384) return "console-error";
        // Match only browser-owned markers; the original message never enters an event.
        if (message.contains("Cross-Origin-Resource-Policy")
            || message.contains("ERR_BLOCKED_BY_RESPONSE.NotSameOrigin")) {
            return "corp-blocked";
        }
        if (message.contains("CORS policy") || message.contains("Access-Control-Allow-Origin")) {
            return "cors-blocked";
        }
        if (message.contains("Failed to load resource") && message.contains("net::ERR_")) {
            return "net-failed";
        }
        return "console-error";
    }

    private static Set<String> set(String... values) {
        return new HashSet<>(Arrays.asList(values));
    }
}
