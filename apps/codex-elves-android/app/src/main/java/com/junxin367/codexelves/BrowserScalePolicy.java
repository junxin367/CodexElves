package com.junxin367.codexelves;

/** Keeps browser scale calculations independent from WebView callbacks. */
final class BrowserScalePolicy {
    private BrowserScalePolicy() { }

    /**
     * Returns the factor needed to fit the measured page width into the WebView.
     *
     * <p>{@link android.webkit.WebView#getScale()} includes the current device
     * density, while DOM dimensions are reported in CSS pixels. Calculating a
     * relative factor keeps the result correct on both low- and high-density
     * devices.</p>
     */
    static float widthFitZoomFactor(int viewportWidth, int contentWidth, float currentScale) {
        if (viewportWidth <= 0 || contentWidth <= 0
            || !Float.isFinite(currentScale) || currentScale <= 0f) {
            return 1f;
        }
        double renderedWidth = contentWidth * (double) currentScale;
        if (renderedWidth <= viewportWidth) return 1f;
        double factor = viewportWidth / renderedWidth;
        return (float) Math.max(0.01d, Math.min(1d, factor));
    }

    static int widthFitScalePercent(int viewportWidth, int contentWidth, float currentScale) {
        float factor = widthFitZoomFactor(viewportWidth, contentWidth, currentScale);
        return Math.max(1, Math.min(100, Math.round(factor * 100f)));
    }

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
