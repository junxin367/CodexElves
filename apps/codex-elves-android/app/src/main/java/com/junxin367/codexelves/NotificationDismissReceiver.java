package com.junxin367.codexelves;

import android.content.BroadcastReceiver;
import android.content.Context;
import android.content.Intent;

/** Reached only by explicit immutable notification delete intents inside this application. */
public final class NotificationDismissReceiver extends BroadcastReceiver {
    @Override
    public void onReceive(Context context, Intent intent) {
        NotificationPublisher.recordUserDismiss(context, intent);
    }
}
