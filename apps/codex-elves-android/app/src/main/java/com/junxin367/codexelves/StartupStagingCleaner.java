package com.junxin367.codexelves;

import java.io.File;
import java.io.IOException;
import java.nio.file.Files;
import java.util.concurrent.atomic.AtomicBoolean;

final class StartupStagingCleaner {
    private static final String STAGING_PREFIX = ".staging-";

    private StartupStagingCleaner() {
    }

    static int cleanupOnce(File cacheRoot, AtomicBoolean processGate) {
        if (!processGate.compareAndSet(false, true)) {
            return 0;
        }
        return cleanupAtProcessStart(cacheRoot);
    }

    static int cleanupAtProcessStart(File cacheRoot) {
        if (cacheRoot == null || !cacheRoot.isDirectory()) {
            return 0;
        }
        int removed = 0;
        File[] originDirectories = cacheRoot.listFiles();
        if (originDirectories == null) {
            return 0;
        }
        for (File originDirectory : originDirectories) {
            if (!originDirectory.isDirectory() || !isContained(cacheRoot, originDirectory)) {
                continue;
            }
            File[] children = originDirectory.listFiles();
            if (children == null) {
                continue;
            }
            for (File child : children) {
                if (child.isDirectory()
                    && child.getName().startsWith(STAGING_PREFIX)
                    && isContained(cacheRoot, child)) {
                    deleteRecursively(child);
                    if (!child.exists()) {
                        removed++;
                    }
                }
            }
        }
        return removed;
    }

    private static boolean isContained(File root, File candidate) {
        try {
            String rootPath = root.getCanonicalPath() + File.separator;
            return candidate.getCanonicalPath().startsWith(rootPath);
        } catch (IOException error) {
            return false;
        }
    }

    private static void deleteRecursively(File target) {
        if (Files.isSymbolicLink(target.toPath())) {
            target.delete();
            return;
        }
        File[] children = target.listFiles();
        if (children != null) {
            for (File child : children) {
                deleteRecursively(child);
            }
        }
        target.delete();
    }
}
