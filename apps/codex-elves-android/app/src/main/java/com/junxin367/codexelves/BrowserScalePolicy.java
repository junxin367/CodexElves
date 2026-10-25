package com.junxin367.codexelves;

/** Keeps the full-page fitting calculation independent from WebView callbacks. */
final class BrowserScalePolicy {
    private BrowserScalePolicy() { }

    static int fullPageScalePercent(
        int viewportWidth, int viewportHeight, int contentWidth, int contentHeight
    ) {
        if (viewportWidth <= 0 || viewportHeight <= 0 || contentWidth <= 0 || contentHeight <= 0) {
            return 100;
        }
        int viewportMin = Math.min(viewportWidth, viewportHeight);
        int contentMax = Math.max(contentWidth, contentHeight);
        if (contentMax <= viewportMin) return 100;
        int percent = (int) Math.floor(viewportMin * 100d / contentMax);
        return Math.max(1, Math.min(99, percent));
    }
}
