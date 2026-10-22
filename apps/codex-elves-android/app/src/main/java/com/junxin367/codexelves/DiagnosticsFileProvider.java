package com.junxin367.codexelves;

import androidx.core.content.FileProvider;

/** Diagnostic exports have no access to the separate APK update directory. */
public final class DiagnosticsFileProvider extends FileProvider {
    public DiagnosticsFileProvider() {
        super();
    }
}
