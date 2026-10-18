package com.junxin367.codexelves;

import android.os.Build;
import android.os.Looper;
import android.os.SystemClock;
import android.view.HapticFeedbackConstants;
import android.view.View;

final class HapticFeedbackController {
    private static final long FEEDBACK_INTERVAL_MS = 50L;
    private static final long DRAG_TICK_INTERVAL_MS = 80L;

    private boolean foreground;
    private long lastFeedbackAt = -1L;
    private long lastDragTickAt = -1L;

    void setForeground(boolean value) {
        foreground = value;
    }

    static boolean isRequestType(String type) {
        return "haptic-click".equals(type)
            || "haptic-drag-start".equals(type)
            || "haptic-drag-tick".equals(type)
            || "haptic-drag-end".equals(type);
    }

    void perform(View view, String type) {
        if (!isRequestType(type) || !foreground || view == null
            || Looper.myLooper() != Looper.getMainLooper()
            || !view.isAttachedToWindow() || !view.isShown() || !view.isEnabled()
            || view.getWindowVisibility() != View.VISIBLE || !view.hasWindowFocus()) {
            return;
        }
        boolean dragTick = "haptic-drag-tick".equals(type);
        long now = SystemClock.uptimeMillis();
        if ((lastFeedbackAt >= 0L && now - lastFeedbackAt < FEEDBACK_INTERVAL_MS)
            || (dragTick && lastDragTickAt >= 0L
                && now - lastDragTickAt < DRAG_TICK_INTERVAL_MS)) {
            return;
        }
        view.performHapticFeedback(feedbackConstant(type), 0);
        lastFeedbackAt = now;
        if (dragTick) {
            lastDragTickAt = now;
        }
    }

    private static int feedbackConstant(String type) {
        switch (type) {
            case "haptic-click":
                return HapticFeedbackConstants.CONTEXT_CLICK;
            case "haptic-drag-start":
                return Build.VERSION.SDK_INT >= 34
                    ? HapticFeedbackConstants.DRAG_START : HapticFeedbackConstants.LONG_PRESS;
            case "haptic-drag-tick":
                return Build.VERSION.SDK_INT >= 34
                    ? HapticFeedbackConstants.SEGMENT_TICK : HapticFeedbackConstants.CLOCK_TICK;
            case "haptic-drag-end":
                if (Build.VERSION.SDK_INT >= 30) {
                    return HapticFeedbackConstants.GESTURE_END;
                }
                return Build.VERSION.SDK_INT >= 27
                    ? HapticFeedbackConstants.VIRTUAL_KEY_RELEASE : HapticFeedbackConstants.VIRTUAL_KEY;
            default:
                throw new IllegalArgumentException("Unknown haptic request type");
        }
    }
}
