package com.junxin367.codexelves;

import java.net.URI;
import java.net.URISyntaxException;
import java.util.function.Consumer;
import java.util.function.LongSupplier;
import java.util.regex.Pattern;

final class ZrokCookiePolicy {
    static final String HOST_SUFFIX = ".shares.zrok.io";
    static final String COOKIE =
        "zrok_interstitial=1; Path=/; Max-Age=604800; Secure; HttpOnly; SameSite=Strict";
    private static final Pattern SHARE_LABEL =
        Pattern.compile("[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?");

    private ZrokCookiePolicy() {
    }

    static boolean appliesTo(String normalizedOrigin) {
        if (normalizedOrigin == null) {
            return false;
        }
        try {
            URI uri = new URI(normalizedOrigin);
            String host = uri.getHost();
            if (!"https".equals(uri.getScheme())
                || host == null
                || host.length() <= HOST_SUFFIX.length()
                || !host.endsWith(HOST_SUFFIX)) {
                return false;
            }
            String shareLabel = host.substring(0, host.length() - HOST_SUFFIX.length());
            return SHARE_LABEL.matcher(shareLabel).matches();
        } catch (URISyntaxException error) {
            return false;
        }
    }

    static void prepare(
        String normalizedOrigin,
        long generation,
        LongSupplier currentGeneration,
        CookieWriter writer,
        Consumer<Boolean> completion
    ) {
        if (currentGeneration.getAsLong() != generation) {
            return;
        }
        if (!appliesTo(normalizedOrigin)) {
            completion.accept(true);
            return;
        }
        writer.setCookie(normalizedOrigin, COOKIE, accepted -> {
            if (currentGeneration.getAsLong() == generation) {
                completion.accept(Boolean.TRUE.equals(accepted));
            }
        });
    }

    interface CookieWriter {
        void setCookie(String origin, String cookie, Consumer<Boolean> callback);
    }
}
