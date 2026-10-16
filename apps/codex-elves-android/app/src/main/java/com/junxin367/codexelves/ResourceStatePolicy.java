package com.junxin367.codexelves;

final class ResourceStatePolicy {
    private ResourceStatePolicy() {
    }

    static Recovery recoverInterruptedTrial(String active, String previous, String trial) {
        if (trial == null || trial.isEmpty()) {
            return new Recovery(active, false);
        }
        String recovered = previous == null || previous.isEmpty()
            ? ResourceStore.BUILTIN
            : previous;
        return new Recovery(recovered, true);
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
        final boolean clearTrial;

        Recovery(String active, boolean clearTrial) {
            this.active = active;
            this.clearTrial = clearTrial;
        }
    }
}
