package com.junxin367.codexelves;

import org.json.JSONArray;
import org.json.JSONException;
import org.json.JSONObject;

import java.util.ArrayList;
import java.util.HashSet;
import java.util.Iterator;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Set;

/** Network-thread state. A cursor advances only after its metadata has been handled. */
final class NotificationEventState {
    static final long MAX_SAFE_INTEGER = 9_007_199_254_740_991L;
    private static final int MAX_SEEN = 1024;
    private String epoch;
    private long cursor;
    private final LinkedHashMap<String, Boolean> seen = new LinkedHashMap<>();
    private final Map<String, Event> pending = new LinkedHashMap<>();

    interface Sink {
        boolean notify(Event event, boolean previouslySeen);
        void dismiss(String notificationId);
        void reset();
        void reconcilePending(Set<String> notificationIds);
    }

    static final class Event {
        final String epoch;
        final long sequence;
        final String id;
        final String notificationId;
        final String action;
        final String kind;
        final String threadId;
        final String title;

        Event(JSONObject value) throws JSONException {
            epoch = epoch(value, "epoch");
            sequence = integer(value, "sequence");
            id = text(value, "id", 200);
            notificationId = text(value, "notificationId", 200);
            action = text(value, "action", 7);
            kind = text(value, "kind", 9);
            threadId = text(value, "threadId", 120);
            if (sequence == 0 || !id.equals(epoch + ":" + sequence)
                || !validNotificationId(notificationId, epoch, sequence)
                || !("notify".equals(action) || "dismiss".equals(action))
                || !("approval".equals(kind) || "question".equals(kind)
                    || "completed".equals(kind) || "failed".equals(kind))
                || "dismiss".equals(action)
                    && (!("approval".equals(kind) || "question".equals(kind))
                        || id.equals(notificationId))
                || !NotificationIdentity.validThreadId(threadId)
                || "notify".equals(action) && !id.equals(notificationId)) {
                throw new JSONException("Invalid notification event");
            }
            Object rawTitle = value.opt("title");
            if (!(rawTitle instanceof String)
                || ((String) rawTitle).codePointCount(0, ((String) rawTitle).length()) > 160) {
                throw new JSONException("Invalid notification title");
            }
            StringBuilder clean = new StringBuilder();
            ((String) rawTitle).codePoints().filter(code ->
                !Character.isISOControl(code)
                    && Character.getType(code) != Character.FORMAT
                    && code != 0x2028 && code != 0x2029
            ).limit(160).forEach(clean::appendCodePoint);
            title = clean.toString().trim();
            integer(value, "createdAtMs");
        }

        boolean isPending() {
            return "approval".equals(kind) || "question".equals(kind);
        }

        private static boolean validNotificationId(String id, String epoch, long sequence) {
            String prefix = epoch + ":";
            if (!id.startsWith(prefix)) {
                return false;
            }
            String suffix = id.substring(prefix.length());
            if (!suffix.matches("[1-9][0-9]{0,15}")) {
                return false;
            }
            try {
                long number = Long.parseLong(suffix);
                return number <= sequence && number <= MAX_SAFE_INTEGER;
            } catch (NumberFormatException error) {
                return false;
            }
        }
    }

    JSONObject request() throws JSONException {
        JSONObject request = new JSONObject();
        if (epoch != null) {
            request.put("epoch", epoch).put("cursor", cursor);
        }
        return request;
    }

    void restore(JSONObject stored) {
        try {
            String restoredEpoch = epoch(stored, "epoch");
            long restoredCursor = integer(stored, "cursor");
            JSONArray restoredSeen = stored.getJSONArray("seen");
            if (restoredSeen.length() > MAX_SEEN + 2048) {
                return;
            }
            LinkedHashMap<String, Boolean> accepted = new LinkedHashMap<>();
            for (int index = 0; index < restoredSeen.length(); index++) {
                Object id = restoredSeen.get(index);
                if (!(id instanceof String) || ((String) id).length() > 200
                    || !((String) id).startsWith(restoredEpoch + ":")) {
                    return;
                }
                accepted.put((String) id, true);
            }
            epoch = restoredEpoch;
            cursor = restoredCursor;
            seen.putAll(accepted);
        } catch (JSONException ignored) {
            // A missing or invalid journal establishes a fresh baseline.
        }
    }

    JSONObject journal() throws JSONException {
        if (epoch == null) {
            return new JSONObject();
        }
        return new JSONObject().put("epoch", epoch).put("cursor", cursor)
            .put("seen", new JSONArray(seen.keySet()));
    }

    void snapshot(JSONObject value, Sink sink) throws JSONException {
        if (!Boolean.TRUE.equals(value.opt("ok"))
            || integer(value, "schemaVersion") != 1
            || !(value.opt("reset") instanceof Boolean)
            || !(value.opt("truncated") instanceof Boolean)) {
            throw new JSONException("Invalid notification snapshot");
        }
        String nextEpoch = epoch(value, "notificationEpoch");
        long nextCursor = integer(value, "notificationCursor");
        List<Event> nextPending = events(value.getJSONArray("pending"), nextEpoch, nextCursor, false);
        List<Event> events = events(value.getJSONArray("events"), nextEpoch, nextCursor, true);
        Set<String> pendingIds = new HashSet<>();
        for (Event event : nextPending) {
            if (!event.isPending() || !"notify".equals(event.action)
                || !pendingIds.add(event.notificationId)) {
                throw new JSONException("Invalid pending notification");
            }
        }
        boolean reset = value.getBoolean("reset") || !nextEpoch.equals(epoch);
        if (!reset && nextCursor < cursor) {
            throw new JSONException("Notification cursor moved backwards");
        }
        if (reset) {
            sink.reset();
            epoch = nextEpoch;
            cursor = 0;
            seen.clear();
            pending.clear();
        }
        sink.reconcilePending(pendingIds);
        // The complete pending snapshot is authoritative, including truncated catch-up.
        for (String id : new HashSet<>(pending.keySet())) {
            if (!pendingIds.contains(id)) {
                sink.dismiss(id);
                pending.remove(id);
            }
        }
        pending.clear();
        for (Event event : nextPending) {
            pending.put(event.notificationId, event);
        }
        if (!reset) {
            for (Event event : events) {
                if (event.sequence <= cursor) {
                    continue;
                }
                if (event.isPending() && "notify".equals(event.action)
                    && !pendingIds.contains(event.notificationId)) {
                    sink.dismiss(event.notificationId);
                    remember(event.notificationId);
                } else {
                    apply(event, sink);
                }
                cursor = event.sequence;
            }
        }
        for (Event event : nextPending) {
            notify(event, sink);
        }
        cursor = nextCursor;
    }

    /** False requests catch-up before accepting an epoch change or a sequence gap. */
    boolean stream(Event event, Sink sink) {
        if (!event.epoch.equals(epoch) || event.sequence > cursor + 1) {
            return false;
        }
        if (event.sequence <= cursor) {
            return true;
        }
        apply(event, sink);
        cursor = event.sequence;
        return true;
    }

    void deleteThread(String threadId, Sink sink) {
        Iterator<Map.Entry<String, Event>> entries = pending.entrySet().iterator();
        while (entries.hasNext()) {
            Map.Entry<String, Event> entry = entries.next();
            if (threadId.equals(entry.getValue().threadId)) {
                sink.dismiss(entry.getKey());
                entries.remove();
            }
        }
    }

    private void apply(Event event, Sink sink) {
        if ("dismiss".equals(event.action)) {
            pending.remove(event.notificationId);
            sink.dismiss(event.notificationId);
            remember(event.notificationId);
        } else {
            if (event.isPending()) {
                pending.put(event.notificationId, event);
            }
            notify(event, sink);
        }
    }

    private void notify(Event event, Sink sink) {
        if (sink.notify(event, seen.containsKey(event.notificationId))) {
            remember(event.notificationId);
        }
    }

    private void remember(String id) {
        seen.put(id, true);
        Iterator<String> entries = seen.keySet().iterator();
        while (seen.size() > MAX_SEEN + pending.size() && entries.hasNext()) {
            if (!pending.containsKey(entries.next())) {
                entries.remove();
            }
        }
    }

    private static List<Event> events(JSONArray values, String epoch, long cursor, boolean ordered)
        throws JSONException {
        if (values.length() > 2048) {
            throw new JSONException("Notification snapshot too large");
        }
        List<Event> events = new ArrayList<>();
        long previous = 0;
        for (int index = 0; index < values.length(); index++) {
            Event event = new Event(values.getJSONObject(index));
            if (!epoch.equals(event.epoch) || event.sequence > cursor
                || ordered && event.sequence <= previous) {
                throw new JSONException("Invalid notification event ordering");
            }
            previous = event.sequence;
            events.add(event);
        }
        return events;
    }

    static long integer(JSONObject value, String field) throws JSONException {
        Object item = value.opt(field);
        if (!(item instanceof Number)) {
            throw new JSONException("Invalid notification number");
        }
        double number = ((Number) item).doubleValue();
        if (!Double.isFinite(number) || number < 0 || number > MAX_SAFE_INTEGER
            || number != Math.floor(number)) {
            throw new JSONException("Invalid notification number");
        }
        return ((Number) item).longValue();
    }

    private static String epoch(JSONObject value, String field) throws JSONException {
        String epoch = text(value, field, 120);
        if (!epoch.matches("[a-zA-Z0-9_-]{1,120}")) {
            throw new JSONException("Invalid notification epoch");
        }
        return epoch;
    }

    private static String text(JSONObject value, String field, int limit) throws JSONException {
        Object item = value.opt(field);
        if (!(item instanceof String) || ((String) item).isEmpty()
            || ((String) item).length() > limit) {
            throw new JSONException("Invalid notification text");
        }
        String text = (String) item;
        for (int index = 0; index < text.length(); index++) {
            if (Character.isISOControl(text.charAt(index))) {
                throw new JSONException("Invalid notification text");
            }
        }
        return text;
    }
}
