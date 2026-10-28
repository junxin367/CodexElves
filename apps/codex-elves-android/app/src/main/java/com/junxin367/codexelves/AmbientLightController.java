package com.junxin367.codexelves;

import android.app.Activity;
import android.hardware.Sensor;
import android.hardware.SensorEvent;
import android.hardware.SensorEventListener;
import android.hardware.SensorManager;
import android.os.SystemClock;

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
    private Float lastLux;
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
        if (needed) manager.registerListener(this, sensor, SensorManager.SENSOR_DELAY_NORMAL);
        else manager.unregisterListener(this);
        registered = needed;
    }

    @Override public void onSensorChanged(SensorEvent event) {
        if (event == null || event.values == null || event.values.length == 0) return;
        float lux = event.values[0];
        if (!Float.isFinite(lux) || lux < 0) return;
        lastLux = lux;
        long now = SystemClock.elapsedRealtime();
        if (lastPublishedLux != null && Math.abs(lastPublishedLux - lux) < 2f && now - lastPublishedAtMs < 2000L)
            return;
        lastPublishedLux = lux;
        lastPublishedAtMs = now;
        listener.update(true, lux);
    }

    @Override public void onAccuracyChanged(Sensor changedSensor, int accuracy) { }
}
