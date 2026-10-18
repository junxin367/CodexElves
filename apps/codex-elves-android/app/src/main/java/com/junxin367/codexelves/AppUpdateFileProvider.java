package com.junxin367.codexelves;

import androidx.core.content.FileProvider;

/** Separate authority and path configuration for private, verified update APKs. */
public final class AppUpdateFileProvider extends FileProvider {
    public AppUpdateFileProvider() {
        super();
    }
}
