package com.junxin367.codexelves;

import android.app.Activity;
import android.hardware.Sensor;
import android.hardware.SensorEvent;
import android.hardware.SensorEventListener;
import android.hardware.SensorManager;
import android.os.SystemClock;

import org.json.JSONObject;

/** Lifecycle-aware ambient light subscription; no sensor permission is required. */
final class AmbientLightController implements SensorEventListener {
    interface Listener {
        void update(boolean supported, Float lux);
    }

    private final SensorManager manager;
    private final Sensor sensor;
    private final Listener listener;
    private int subscriptions;
    private boolean resumed = true;
    private boolean registered;
    private boolean registrationSucceeded;
    private Float lastLux;
    private long lastSampleAtMs = -1;
    private long lastPublishedAtMs;
    private Float lastPublishedLux;

    AmbientLightController(Activity activity, Listener listener) {
        manager = (SensorManager) activity.getSystemService(Activity.SENSOR_SERVICE);
        sensor = manager == null ? null : manager.getDefaultSensor(Sensor.TYPE_LIGHT);
        this.listener = listener;
    }

    synchronized int subscribe() {
        subscriptions++;
        updateRegistration();
        if (sensor == null) listener.update(false, null);
        else if (lastLux != null) listener.update(true, lastLux);
        return subscriptions;
    }

    synchronized int unsubscribe() {
        subscriptions = Math.max(0, subscriptions - 1);
        updateRegistration();
        return subscriptions;
    }

    synchronized void onResume() {
        resumed = true;
        updateRegistration();
    }

    synchronized void onPause() {
        resumed = false;
        updateRegistration();
    }

    synchronized void clear() {
        subscriptions = 0;
        updateRegistration();
    }

    private void updateRegistration() {
        boolean needed = subscriptions > 0 && resumed && sensor != null;
        if (needed == registered || manager == null) return;
        // 用户要求先采证；单独保存实际注册结果，保留原有注册、重试和生命周期判断。
        if (needed) registrationSucceeded = manager.registerListener(this, sensor, SensorManager.SENSOR_DELAY_NORMAL);
        else {
            manager.unregisterListener(this);
            registrationSucceeded = false;
        }
        registered = needed;
    }

    synchronized Long sampleAgeMs() {
        return lastSampleAtMs < 0 ? null : Math.max(0, SystemClock.elapsedRealtime() - lastSampleAtMs);
    }

    synchronized JSONObject diagnosticFields() {
        return DiagnosticsPolicy.fields(
            "supported", sensor != null, "registered", registrationSucceeded,
            "visible", resumed, "count", subscriptions,
            "lux", lastLux == null ? null : lastLux.doubleValue(), "sampleAgeMs", sampleAgeMs()
        );
    }

    @Override public void onSensorChanged(SensorEvent event) {
        if (event == null || event.values == null || event.values.length == 0) return;
        float lux = event.values[0];
        if (!Float.isFinite(lux) || lux < 0) return;
        lastLux = lux;
        long now = SystemClock.elapsedRealtime();
        // 记录真实采样时间，节流与缓存回传不能把旧读数记成新采样。
        lastSampleAtMs = now;
        if (lastPublishedLux != null && Math.abs(lastPublishedLux - lux) < 2f && now - lastPublishedAtMs < 2000L)
            return;
        lastPublishedLux = lux;
        lastPublishedAtMs = now;
        listener.update(true, lux);
    }

    @Override public void onAccuracyChanged(Sensor changedSensor, int accuracy) { }
}
