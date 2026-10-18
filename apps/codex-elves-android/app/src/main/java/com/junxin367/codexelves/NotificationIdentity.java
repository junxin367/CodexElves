package com.junxin367.codexelves;

import java.net.URI;
import java.net.URISyntaxException;
import java.nio.charset.StandardCharsets;
import java.security.MessageDigest;
import java.security.NoSuchAlgorithmException;
import java.util.Locale;
import java.util.regex.Pattern;

/** An identity contains a device cookie, never a service login key. */
final class NotificationIdentity {
    private static final String COOKIE_NAME = "codex_elves_device";
    private static final Pattern THREAD_ID = Pattern.compile("[a-zA-Z0-9_-]{1,120}");
    final String origin;
    final String digest;
    private final String cookie;

    private NotificationIdentity(String origin, String cookie) {
        this.origin = origin;
        this.cookie = cookie;
        digest = digest(origin + "\n" + cookie);
    }

    static NotificationIdentity create(String origin, String cookieHeader) {
        if (!isCanonicalOrigin(origin) || cookieHeader == null || cookieHeader.length() > 32768) {
            return null;
        }
        String device = null;
        for (String part : cookieHeader.split(";")) {
            int separator = part.indexOf('=');
            if (separator < 0 || !COOKIE_NAME.equals(part.substring(0, separator).trim())) {
                continue;
            }
            if (device != null) {
                return null;
            }
            String value = part.substring(separator + 1).trim();
            if (value.isEmpty() || value.length() > 4096) {
                return null;
            }
            for (int index = 0; index < value.length(); index++) {
                char ch = value.charAt(index);
                if (ch < 0x21 || ch > 0x7e || ch == '"' || ch == ',' || ch == '\\') {
                    return null;
                }
            }
            device = COOKIE_NAME + "=" + value;
        }
        return device == null ? null : new NotificationIdentity(origin, device);
    }

    String requestCookie() {
        return ZrokCookiePolicy.appliesTo(origin) ? cookie + "; zrok_interstitial=1" : cookie;
    }

    static boolean validThreadId(String threadId) {
        return threadId != null && THREAD_ID.matcher(threadId).matches();
    }

    static boolean validDigest(String digest) {
        return digest != null && digest.matches("[0-9a-f]{64}");
    }

    private static boolean isCanonicalOrigin(String origin) {
        if (origin == null || origin.length() > 2048) {
            return false;
        }
        try {
            URI uri = new URI(origin);
            String host = uri.getHost();
            int port = uri.getPort();
            if (!"https".equals(uri.getScheme()) || host == null || host.isEmpty()
                || uri.getRawUserInfo() != null || uri.getRawQuery() != null
                || uri.getRawFragment() != null
                || uri.getRawPath() != null && !uri.getRawPath().isEmpty()
                || port == 0 || port < -1 || port > 65535 || port == 443) {
                return false;
            }
            return origin.equals(new URI(
                "https", null, host.toLowerCase(Locale.ROOT), port, null, null, null
            ).toASCIIString());
        } catch (URISyntaxException error) {
            return false;
        }
    }

    private static String digest(String value) {
        try {
            byte[] bytes = MessageDigest.getInstance("SHA-256")
                .digest(value.getBytes(StandardCharsets.UTF_8));
            StringBuilder encoded = new StringBuilder(64);
            for (byte item : bytes) {
                encoded.append(Character.forDigit((item >>> 4) & 15, 16));
                encoded.append(Character.forDigit(item & 15, 16));
            }
            return encoded.toString();
        } catch (NoSuchAlgorithmException error) {
            throw new IllegalStateException("SHA-256 unavailable", error);
        }
    }
}
