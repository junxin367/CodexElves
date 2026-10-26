package com.junxin367.codexelves;

final class ResourceStatePolicy {
    private ResourceStatePolicy() {
    }

    static boolean shouldPreferBundled(String recognizedBundledVersion, String bundledVersion) {
        return !bundledVersion.equals(recognizedBundledVersion);
    }

    static Recovery recoverInterruptedTrial(String active, String previous, String trial) {
        if (trial == null || trial.isEmpty()) {
            return new Recovery(active, "", false);
        }
        String recovered = previous == null || previous.isEmpty()
            ? ResourceStore.BUILTIN
            : previous;
        // Keep the verified candidate as pending so an interrupted startup
        // retries the existing directory instead of downloading it again.
        return new Recovery(recovered, trial, true);
    }

    static String pendingAfterDownload(
        String existingPending,
        String candidate,
        boolean fullyVerified,
        boolean stillCurrent
    ) {
        if (!fullyVerified || !stillCurrent) {
            return existingPending == null ? "" : existingPending;
        }
        return candidate;
    }

    static final class Recovery {
        final String active;
        final String pending;
        final boolean clearTrial;

        Recovery(String active, String pending, boolean clearTrial) {
            this.active = active;
            this.pending = pending;
            this.clearTrial = clearTrial;
        }
    }
}
