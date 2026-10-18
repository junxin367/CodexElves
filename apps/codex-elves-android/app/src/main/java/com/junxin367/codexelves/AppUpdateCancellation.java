package com.junxin367.codexelves;

import java.io.InterruptedIOException;
import java.net.HttpURLConnection;
import java.util.concurrent.TimeUnit;

/** One request owns its connections, deadline and cancellation state. */
final class AppUpdateCancellation {
    private final long deadlineNanos;
    private volatile boolean cancelled;
    private HttpURLConnection connection;

    AppUpdateCancellation(long timeoutMillis) {
        deadlineNanos = System.nanoTime() + TimeUnit.MILLISECONDS.toNanos(timeoutMillis);
    }

    void check() throws InterruptedIOException {
        if (cancelled || Thread.currentThread().isInterrupted()) {
            throw new InterruptedIOException("更新已取消");
        }
        if (System.nanoTime() >= deadlineNanos) {
            throw new InterruptedIOException("更新请求超时");
        }
    }

    int timeout(int maximumMillis) throws InterruptedIOException {
        check();
        long remaining = TimeUnit.NANOSECONDS.toMillis(deadlineNanos - System.nanoTime());
        return (int) Math.max(1L, Math.min(maximumMillis, remaining));
    }

    synchronized void attach(HttpURLConnection next) throws InterruptedIOException {
        check();
        connection = next;
    }

    synchronized void detach(HttpURLConnection previous) {
        if (connection == previous) {
            connection = null;
        }
    }

    boolean isCancelled() {
        return cancelled;
    }

    void cancel() {
        HttpURLConnection active;
        synchronized (this) {
            cancelled = true;
            active = connection;
            connection = null;
        }
        if (active != null) {
            active.disconnect();
        }
    }
}
