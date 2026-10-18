package com.junxin367.codexelves;

import android.app.Activity;
import android.app.AlertDialog;
import android.content.ActivityNotFoundException;
import android.content.ClipData;
import android.content.Intent;
import android.content.SharedPreferences;
import android.net.Uri;
import android.os.Handler;
import android.os.Looper;
import android.provider.Settings;
import android.view.ViewGroup;
import android.widget.LinearLayout;
import android.widget.ProgressBar;
import android.widget.TextView;

import androidx.core.content.FileProvider;

import java.io.IOException;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.Future;
import java.util.concurrent.TimeUnit;

/** Activity-owned UI; every completion is checked against its original operation. */
final class AppUpdateController {
    // File chooser request codes start at 4107 and wrap within 4107..65535.
    private static final int INSTALL_SOURCE_REQUEST = 4001;
    private static final long CHECK_INTERVAL_MS = TimeUnit.HOURS.toMillis(6);
    private static final long SAME_VERSION_PROMPT_INTERVAL_MS = TimeUnit.DAYS.toMillis(1);
    private static final long CHECK_TIMEOUT_MS = TimeUnit.SECONDS.toMillis(90);
    private static final long DOWNLOAD_TIMEOUT_MS = TimeUnit.MINUTES.toMillis(10);
    private static final long VERIFY_TIMEOUT_MS = TimeUnit.MINUTES.toMillis(1);

    private final Activity activity;
    private final AppUpdateClient client;
    private final SharedPreferences preferences;
    private final Handler main = new Handler(Looper.getMainLooper());
    private final ExecutorService executor = Executors.newSingleThreadExecutor(
        runnable -> new Thread(runnable, "codex-elves-app-update")
    );
    private final Runnable automaticCheck = () -> check(false);

    private boolean foreground;
    private boolean destroyed;
    private Operation active;
    private AppUpdatePolicy.Update offer;
    private boolean offerManual;
    private PendingInstall pendingInstall;
    private Notice notice;
    private AlertDialog dialog;
    private TextView progressText;
    private ProgressBar progressBar;

    AppUpdateController(Activity activity) {
        this.activity = activity;
        client = new AppUpdateClient(activity);
        preferences = activity.getSharedPreferences("android-app-update", Activity.MODE_PRIVATE);
    }

    void onResume() {
        if (destroyed) {
            return;
        }
        foreground = true;
        if (pendingInstall != null && pendingInstall.waitingPermission) {
            permissionReturned();
        }
        render();
        main.removeCallbacks(automaticCheck);
        main.postDelayed(automaticCheck, 2_000L);
    }

    void onPause() {
        foreground = false;
        main.removeCallbacks(automaticCheck);
        dismissDialog();
    }

    void onDestroy() {
        destroyed = true;
        foreground = false;
        main.removeCallbacks(automaticCheck);
        cancelActive();
        if (pendingInstall != null && !pendingInstall.handedToInstaller) {
            pendingInstall.downloaded.delete();
        }
        pendingInstall = null;
        offer = null;
        notice = null;
        executor.shutdownNow();
        dismissDialog();
    }

    boolean onActivityResult(int requestCode) {
        if (requestCode != INSTALL_SOURCE_REQUEST) {
            return false;
        }
        permissionReturned();
        render();
        return true;
    }

    void checkManually() {
        check(true);
    }

    private void check(boolean manual) {
        if (destroyed || activity.isFinishing() || !manual && !foreground) {
            return;
        }
        if (BuildConfig.DEBUG) {
            if (manual) {
                notice = new Notice(R.string.app_update_debug_unsupported, null);
                render();
            }
            return;
        }
        if (active != null) {
            active.manual |= manual;
            render();
            return;
        }
        if (pendingInstall != null) {
            if (manual) {
                pendingInstall.requested = true;
                pendingInstall.readyToLaunch = false;
                dismissDialog();
                render();
            }
            return;
        }
        if (!manual) {
            long now = System.currentTimeMillis();
            long previous = preferences.getLong("last_check_at", 0L);
            if (previous > 0 && previous <= now && now - previous < CHECK_INTERVAL_MS) {
                return;
            }
        }
        preferences.edit().putLong("last_check_at", System.currentTimeMillis()).apply();
        offer = null;
        notice = null;
        dismissDialog();
        Operation operation = new Operation(Kind.CHECK, manual, CHECK_TIMEOUT_MS);
        active = operation;
        operation.future = executor.submit(() -> {
            AppUpdatePolicy.Update update = null;
            boolean failed = false;
            try {
                update = client.check(operation.cancellation);
            } catch (IOException | RuntimeException error) {
                failed = true;
            }
            AppUpdatePolicy.Update result = update;
            boolean failure = failed;
            main.post(() -> completeCheck(operation, result, failure));
        });
        render();
    }

    private void completeCheck(
        Operation operation,
        AppUpdatePolicy.Update result,
        boolean failed
    ) {
        if (!owns(operation)) {
            return;
        }
        active = null;
        dismissDialog();
        if (failed) {
            if (operation.manual) {
                notice = new Notice(R.string.app_update_check_failed, this::checkManually);
            }
        } else if (result != null) {
            offer = result;
            offerManual = operation.manual;
        } else if (operation.manual) {
            notice = new Notice(R.string.app_update_no_update, null);
        }
        render();
    }

    private void download(AppUpdatePolicy.Update update) {
        if (destroyed || active != null) {
            return;
        }
        offer = null;
        notice = null;
        dismissDialog();
        Operation operation = new Operation(Kind.DOWNLOAD, true, DOWNLOAD_TIMEOUT_MS);
        active = operation;
        operation.future = executor.submit(() -> {
            AppUpdateClient.Downloaded downloaded = null;
            try {
                downloaded = client.download(update, operation.cancellation, (bytes, total) -> {
                    int percent = (int) (bytes * 100L / total);
                    if (percent != operation.percent) {
                        operation.percent = percent;
                        main.post(() -> refreshProgress(operation));
                    }
                });
            } catch (IOException | RuntimeException error) {
                // The client deletes partial and rejected APKs before returning.
            }
            AppUpdateClient.Downloaded result = downloaded;
            main.post(() -> {
                if (!owns(operation)) {
                    if (result != null) {
                        result.delete();
                    }
                    return;
                }
                active = null;
                dismissDialog();
                if (result == null) {
                    notice = new Notice(R.string.app_update_download_failed, () -> download(update));
                } else {
                    pendingInstall = new PendingInstall(result);
                }
                render();
            });
        });
        render();
    }

    private void verifyAndInstall(PendingInstall pending) {
        if (destroyed || active != null || pendingInstall != pending) {
            return;
        }
        dismissDialog();
        Operation operation = new Operation(Kind.VERIFY, true, VERIFY_TIMEOUT_MS);
        active = operation;
        operation.future = executor.submit(() -> {
            boolean valid = false;
            try {
                client.verifyDownloaded(pending.downloaded, operation.cancellation);
                valid = true;
            } catch (IOException | RuntimeException error) {
                // No URI grant exists while validation is running.
            }
            boolean accepted = valid;
            main.post(() -> {
                if (!owns(operation) || pendingInstall != pending) {
                    return;
                }
                active = null;
                dismissDialog();
                if (!accepted) {
                    pending.downloaded.delete();
                    pendingInstall = null;
                    notice = new Notice(R.string.app_update_download_failed, this::checkManually);
                } else {
                    pending.readyToLaunch = true;
                }
                render();
            });
        });
        render();
    }

    private void render() {
        if (!canShowUi() || dialog != null) {
            return;
        }
        if (active != null) {
            if (active.kind != Kind.CHECK || active.manual) {
                showProgress(active);
            }
            return;
        }
        if (notice != null) {
            showNotice(notice);
            return;
        }
        if (pendingInstall != null) {
            PendingInstall pending = pendingInstall;
            if (!pending.requested || pending.waitingPermission) {
                return;
            }
            if (!activity.getPackageManager().canRequestPackageInstalls()) {
                showInstallPermission(pending);
            } else if (pending.readyToLaunch) {
                launchInstaller(pending);
            } else {
                verifyAndInstall(pending);
            }
            return;
        }
        if (offer != null) {
            long now = System.currentTimeMillis();
            long lastAt = preferences.getLong("last_prompt_at", 0L);
            if (!offerManual
                && preferences.getLong("last_prompt_code", -1L) == offer.versionCode
                && lastAt > 0 && lastAt <= now
                && now - lastAt < SAME_VERSION_PROMPT_INTERVAL_MS) {
                offer = null;
                return;
            }
            preferences.edit()
                .putLong("last_prompt_code", offer.versionCode)
                .putLong("last_prompt_at", now)
                .apply();
            showOffer(offer);
        }
    }

    private void showOffer(AppUpdatePolicy.Update update) {
        AlertDialog next = new AlertDialog.Builder(activity)
            .setTitle(R.string.app_update_available)
            .setMessage(activity.getString(
                R.string.app_update_offer,
                BuildConfig.VERSION_NAME,
                update.release.versionName
            ))
            .setPositiveButton(R.string.app_update_download, (ignored, which) -> {
                if (offer == update) {
                    download(update);
                }
            })
            .setNegativeButton(R.string.app_update_later, (ignored, which) -> offer = null)
            .setOnCancelListener(ignored -> offer = null)
            .create();
        show(next);
    }

    private void showNotice(Notice current) {
        AlertDialog.Builder builder = new AlertDialog.Builder(activity)
            .setTitle(R.string.app_update_title)
            .setMessage(current.message);
        if (current.retry == null) {
            builder.setPositiveButton(android.R.string.ok, (ignored, which) -> notice = null);
        } else {
            builder.setPositiveButton(R.string.app_update_retry, (ignored, which) -> {
                notice = null;
                dismissDialog();
                current.retry.run();
            });
            builder.setNegativeButton(R.string.app_update_later, (ignored, which) -> notice = null);
        }
        builder.setOnCancelListener(ignored -> notice = null);
        show(builder.create());
    }

    private void showProgress(Operation operation) {
        LinearLayout content = new LinearLayout(activity);
        content.setOrientation(LinearLayout.VERTICAL);
        int padding = Math.round(24 * activity.getResources().getDisplayMetrics().density);
        content.setPadding(padding, padding / 2, padding, padding / 2);
        progressText = new TextView(activity);
        content.addView(progressText);
        progressBar = operation.kind == Kind.DOWNLOAD
            ? new ProgressBar(activity, null, android.R.attr.progressBarStyleHorizontal)
            : new ProgressBar(activity);
        progressBar.setIndeterminate(operation.kind != Kind.DOWNLOAD);
        progressBar.setMax(100);
        content.addView(progressBar, new LinearLayout.LayoutParams(
            ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT
        ));
        AlertDialog next = new AlertDialog.Builder(activity)
            .setTitle(R.string.app_update_title)
            .setView(content)
            .setNegativeButton(android.R.string.cancel, (ignored, which) -> cancelActive())
            .setOnCancelListener(ignored -> cancelActive())
            .create();
        next.setCanceledOnTouchOutside(false);
        show(next);
        refreshProgress(operation);
    }

    private void refreshProgress(Operation operation) {
        if (!owns(operation) || !canShowUi() || progressText == null || progressBar == null) {
            return;
        }
        if (operation.kind == Kind.DOWNLOAD) {
            progressText.setText(activity.getString(R.string.app_update_downloading, operation.percent));
            progressBar.setProgress(operation.percent);
        } else {
            progressText.setText(operation.kind == Kind.CHECK
                ? R.string.app_update_checking : R.string.app_update_verifying);
        }
    }

    private void showInstallPermission(PendingInstall pending) {
        AlertDialog next = new AlertDialog.Builder(activity)
            .setTitle(R.string.app_update_install_permission_title)
            .setMessage(pending.permissionDenied
                ? R.string.app_update_install_permission_denied : R.string.app_update_install_permission)
            .setPositiveButton(R.string.app_update_open_settings, (ignored, which) -> {
                dismissDialog();
                if (!canShowUi() || pendingInstall != pending) {
                    return;
                }
                pending.waitingPermission = true;
                pending.readyToLaunch = false;
                try {
                    activity.startActivityForResult(
                        new Intent(
                            Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES,
                            Uri.parse("package:" + activity.getPackageName())
                        ),
                        INSTALL_SOURCE_REQUEST
                    );
                } catch (ActivityNotFoundException | SecurityException error) {
                    pending.waitingPermission = false;
                    pending.requested = false;
                    notice = new Notice(R.string.app_update_settings_unavailable, () -> {
                        pending.requested = true;
                        render();
                    });
                    render();
                }
            })
            .setNegativeButton(R.string.app_update_later, (ignored, which) -> pending.requested = false)
            .setOnCancelListener(ignored -> pending.requested = false)
            .create();
        show(next);
    }

    private void permissionReturned() {
        if (pendingInstall == null || !pendingInstall.waitingPermission) {
            return;
        }
        pendingInstall.waitingPermission = false;
        pendingInstall.readyToLaunch = false;
        pendingInstall.permissionDenied = !activity.getPackageManager().canRequestPackageInstalls();
    }

    private void launchInstaller(PendingInstall pending) {
        if (!canShowUi() || pendingInstall != pending || !pending.readyToLaunch) {
            return;
        }
        try {
            Uri apk = FileProvider.getUriForFile(
                activity,
                activity.getPackageName() + ".app-updates",
                pending.downloaded.file
            );
            Intent intent = new Intent(Intent.ACTION_VIEW);
            intent.setDataAndType(apk, "application/vnd.android.package-archive");
            intent.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION);
            intent.setClipData(ClipData.newRawUri("CodexElves update", apk));
            pending.requested = false;
            pending.handedToInstaller = true;
            activity.startActivity(intent);
            // The installer owns the temporary grant; keep its APK until cache expiry.
            pendingInstall = null;
        } catch (ActivityNotFoundException | SecurityException | IllegalArgumentException error) {
            pending.handedToInstaller = false;
            pending.requested = false;
            pending.readyToLaunch = false;
            notice = new Notice(R.string.app_update_installer_unavailable, () -> {
                pending.requested = true;
                render();
            });
            render();
        }
    }

    private boolean owns(Operation operation) {
        return !destroyed && active == operation && !operation.cancellation.isCancelled();
    }

    private boolean canShowUi() {
        return !destroyed && foreground && !activity.isFinishing() && !activity.isDestroyed();
    }

    private void cancelActive() {
        Operation previous = active;
        active = null;
        if (previous != null) {
            previous.cancellation.cancel();
            if (previous.future != null) {
                previous.future.cancel(true);
            }
            if (previous.kind == Kind.VERIFY && pendingInstall != null
                && !pendingInstall.handedToInstaller) {
                pendingInstall.downloaded.delete();
                pendingInstall = null;
            }
        }
        dismissDialog();
    }

    private void show(AlertDialog next) {
        if (!canShowUi()) {
            return;
        }
        dialog = next;
        next.setOnDismissListener(ignored -> {
            if (dialog == next) {
                dialog = null;
                progressText = null;
                progressBar = null;
            }
        });
        next.show();
    }

    private void dismissDialog() {
        AlertDialog previous = dialog;
        dialog = null;
        progressText = null;
        progressBar = null;
        if (previous != null) {
            previous.dismiss();
        }
    }

    private enum Kind { CHECK, DOWNLOAD, VERIFY }

    private static final class Operation {
        final Kind kind;
        final AppUpdateCancellation cancellation;
        boolean manual;
        volatile int percent;
        Future<?> future;

        Operation(Kind kind, boolean manual, long timeout) {
            this.kind = kind;
            this.manual = manual;
            cancellation = new AppUpdateCancellation(timeout);
        }
    }

    private static final class PendingInstall {
        final AppUpdateClient.Downloaded downloaded;
        boolean requested = true;
        boolean waitingPermission;
        boolean permissionDenied;
        boolean readyToLaunch;
        boolean handedToInstaller;

        PendingInstall(AppUpdateClient.Downloaded downloaded) {
            this.downloaded = downloaded;
        }
    }

    private static final class Notice {
        final int message;
        final Runnable retry;

        Notice(int message, Runnable retry) {
            this.message = message;
            this.retry = retry;
        }
    }
}
