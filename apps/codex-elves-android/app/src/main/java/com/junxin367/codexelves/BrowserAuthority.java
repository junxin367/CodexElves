package com.junxin367.codexelves;

import android.os.Binder;
import android.os.IBinder;
import android.os.Parcel;
import android.os.Process;
import android.os.RemoteException;
import java.util.UUID;
import java.util.concurrent.ConcurrentHashMap;
import org.json.JSONException;
import org.json.JSONObject;

/** A non-exported, same-UID capability passed directly to the browser Activity. */
final class BrowserAuthority extends Binder {
    private static final String DESCRIPTOR = "com.junxin367.codexelves.BrowserAuthority";
    private static final int SNAPSHOT = FIRST_CALL_TRANSACTION;
    private static final int REGISTER = FIRST_CALL_TRANSACTION + 1;
    private static final int INVALIDATED = FIRST_CALL_TRANSACTION + 2;
    private static final int UNREGISTER = FIRST_CALL_TRANSACTION + 3;
    private static final int DIAGNOSTIC = FIRST_CALL_TRANSACTION + 4;
    static final String EXTRA = "browser-authority";
    record Snapshot(String owner, String origin, String cookie) { }
    interface DiagnosticSink { void record(String name, JSONObject fields); }
    private final Snapshot identity;
    private final DiagnosticSink diagnosticSink;
    private final ConcurrentHashMap<IBinder, IBinder.DeathRecipient> listeners = new ConcurrentHashMap<>();
    private volatile boolean valid = true;

    BrowserAuthority(String origin, String cookie, DiagnosticSink diagnosticSink) {
        identity = new Snapshot(UUID.randomUUID().toString(), ServiceOrigin.normalize(origin), cookie);
        this.diagnosticSink = diagnosticSink;
    }

    boolean valid() { return valid; }
    boolean matches(String origin, String cookie) {
        return valid && identity.origin.equals(origin) && identity.cookie.equals(cookie);
    }

    void invalidate() {
        if (!valid) return;
        valid = false;
        for (IBinder listener : listeners.keySet()) {
            notifyInvalidated(listener);
            removeListener(listener);
        }
    }

    private void removeListener(IBinder listener) {
        IBinder.DeathRecipient death = listeners.remove(listener);
        if (death != null) listener.unlinkToDeath(death, 0);
    }

    private static void notifyInvalidated(IBinder listener) {
        Parcel data = Parcel.obtain();
        try {
            data.writeInterfaceToken(DESCRIPTOR);
            listener.transact(INVALIDATED, data, null, IBinder.FLAG_ONEWAY);
        } catch (RemoteException ignored) { /* Its process is already gone. */ }
        finally { data.recycle(); }
    }

    @Override protected boolean onTransact(int code, Parcel data, Parcel reply, int flags)
        throws RemoteException {
        if (Process.myUid() != Binder.getCallingUid()) throw new SecurityException("Invalid browser caller");
        if (code == INTERFACE_TRANSACTION) { if (reply != null) reply.writeString(DESCRIPTOR); return true; }
        data.enforceInterface(DESCRIPTOR);
        if (code == SNAPSHOT) {
            if (reply == null) return false;
            reply.writeNoException();
            reply.writeInt(valid ? 1 : 0);
            if (valid) {
                reply.writeString(identity.owner); reply.writeString(identity.origin);
                reply.writeString(identity.cookie);
            }
            return true;
        }
        if (code == REGISTER) {
            IBinder listener = data.readStrongBinder();
            if (listener != null) {
                if (!valid) notifyInvalidated(listener);
                else {
                    IBinder.DeathRecipient death = () -> listeners.remove(listener);
                    if (listeners.putIfAbsent(listener, death) == null) {
                        try { listener.linkToDeath(death, 0); }
                        catch (RemoteException gone) { listeners.remove(listener, death); throw gone; }
                    }
                    if (!valid) { removeListener(listener); notifyInvalidated(listener); }
                }
            }
            if (reply != null) reply.writeNoException();
            return true;
        }
        if (code == UNREGISTER) {
            IBinder listener = data.readStrongBinder();
            if (listener != null) removeListener(listener);
            if (reply != null) reply.writeNoException();
            return true;
        }
        if (code == DIAGNOSTIC) {
            String name = data.readString();
            String rawFields = data.readString();
            if (valid && diagnosticSink != null && DiagnosticsPolicy.validName(name)
                && rawFields != null && rawFields.length() <= DiagnosticsPolicy.MAX_EVENT_CHARS) {
                try {
                    JSONObject fields = new JSONObject(rawFields);
                    if (DiagnosticsPolicy.validFields(fields)) diagnosticSink.record(name, fields);
                } catch (JSONException ignored) {
                    // Ignore malformed diagnostics from the browser process.
                }
            }
            return true;
        }
        return false;
    }

    static void recordDiagnostic(IBinder authority, String name, JSONObject fields) {
        if (authority == null || !DiagnosticsPolicy.validName(name)
            || !DiagnosticsPolicy.validFields(fields)) return;
        Parcel data = Parcel.obtain();
        try {
            data.writeInterfaceToken(DESCRIPTOR);
            data.writeString(name);
            data.writeString(fields.toString());
            authority.transact(DIAGNOSTIC, data, null, IBinder.FLAG_ONEWAY);
        } catch (RemoteException | RuntimeException ignored) {
            // Diagnostics must never change browser behavior.
        } finally {
            data.recycle();
        }
    }

    static Snapshot read(IBinder authority) throws RemoteException {
        if (authority == null) throw new RemoteException("Missing browser identity");
        Parcel data = Parcel.obtain(), reply = Parcel.obtain();
        try {
            data.writeInterfaceToken(DESCRIPTOR);
            if (!authority.transact(SNAPSHOT, data, reply, 0)) throw new RemoteException("Invalid browser identity");
            reply.readException();
            if (reply.readInt() != 1) throw new RemoteException("Browser identity expired");
            String owner = reply.readString(), origin = reply.readString(), cookie = reply.readString();
            if (owner == null || origin == null || cookie == null
                || !owner.matches("[a-f0-9-]{36}")
                || !cookie.matches("codex_elves_device=[A-Za-z0-9._~-]{16,512}"))
                throw new RemoteException("Invalid browser identity");
            return new Snapshot(owner, ServiceOrigin.normalize(origin), cookie);
        } finally { data.recycle(); reply.recycle(); }
    }

    static AutoCloseable observe(IBinder authority, Runnable invalidated) throws RemoteException {
        IBinder.DeathRecipient death = invalidated::run;
        Binder listener = new Binder() {
            @Override protected boolean onTransact(int code, Parcel data, Parcel reply, int flags) {
                if (Process.myUid() != Binder.getCallingUid()) return false;
                data.enforceInterface(DESCRIPTOR);
                if (code != INVALIDATED) return false;
                invalidated.run();
                return true;
            }
        };
        authority.linkToDeath(death, 0);
        Parcel data = Parcel.obtain(), reply = Parcel.obtain();
        try {
            data.writeInterfaceToken(DESCRIPTOR); data.writeStrongBinder(listener);
            if (!authority.transact(REGISTER, data, reply, 0)) throw new RemoteException("Invalid browser observer");
            reply.readException();
        } catch (RemoteException | RuntimeException failure) {
            authority.unlinkToDeath(death, 0);
            throw failure;
        } finally { data.recycle(); reply.recycle(); }
        return () -> {
            Parcel removal = Parcel.obtain(), result = Parcel.obtain();
            try {
                removal.writeInterfaceToken(DESCRIPTOR); removal.writeStrongBinder(listener);
                if (authority.isBinderAlive()) authority.transact(UNREGISTER, removal, result, 0);
            } finally { removal.recycle(); result.recycle(); }
            authority.unlinkToDeath(death, 0);
        };
    }
}
