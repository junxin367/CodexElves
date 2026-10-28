package com.junxin367.codexelves;

import android.content.Context;
import android.content.SharedPreferences;
import android.security.keystore.KeyGenParameterSpec;
import android.security.keystore.KeyProperties;
import android.util.Base64;
import org.json.JSONArray;
import org.json.JSONObject;
import java.nio.charset.StandardCharsets;
import java.security.KeyStore;
import java.util.ArrayList;
import java.util.List;
import java.util.UUID;
import javax.crypto.Cipher;
import javax.crypto.KeyGenerator;
import javax.crypto.SecretKey;
import javax.crypto.spec.GCMParameterSpec;

/** Origin-scoped credential records encrypted with a non-exportable Android Keystore key. */
final class BrowserCredentialStore {
    static final int RECORD_LIMIT = 100;
    static final class Entry {
        final String id;
        final String origin;
        final String username;
        final String password;
        final String subaccount;
        final long updatedAtMs;

        Entry(String id, String origin, String username, String password, String subaccount, long updatedAtMs) {
            this.id = id;
            this.origin = origin;
            this.username = username;
            this.password = password;
            this.subaccount = subaccount;
            this.updatedAtMs = updatedAtMs;
        }
    }

    private static final String KEY_ALIAS = "codex-elves-browser-credentials-v1";
    private static final String RECORD_PREFIX = "records:";
    private static final String ENABLED_PREFIX = "enabled:";
    private final SharedPreferences store;

    BrowserCredentialStore(Context context) {
        store = context.getApplicationContext().getSharedPreferences(
            "codex_elves_browser_credentials", Context.MODE_PRIVATE);
    }

    synchronized boolean isRecordingEnabled(String origin) {
        return store.getBoolean(ENABLED_PREFIX + origin, false);
    }

    synchronized void setRecordingEnabled(String origin, boolean enabled) {
        if (!store.edit().putBoolean(ENABLED_PREFIX + origin, enabled).commit())
            throw new IllegalStateException("账号密码记录设置保存失败，请重试");
    }

    synchronized List<Entry> list(String origin) {
        String encrypted = store.getString(RECORD_PREFIX + origin, "");
        List<Entry> result = new ArrayList<>();
        if (encrypted == null || encrypted.isEmpty()) return result;
        try {
            JSONArray values = new JSONArray(decrypt(origin, encrypted));
            for (int index = 0; index < values.length(); index++) {
                JSONObject value = values.optJSONObject(index);
                if (value == null) continue;
                String id = value.optString("id", "");
                String siteOrigin = value.optString("origin", "");
                String username = value.optString("username", "");
                String password = value.optString("password", "");
                String subaccount = value.optString("subaccount", "");
                long updatedAtMs = value.optLong("updatedAtMs", 0L);
                if (id.isEmpty() || siteOrigin.isEmpty() || password.isEmpty() || updatedAtMs <= 0L) continue;
                result.add(new Entry(id, siteOrigin, username, password, subaccount, updatedAtMs));
            }
        } catch (Exception failure) {
            throw new IllegalStateException("账号密码记录无法解密，请检查系统密钥状态", failure);
        }
        return result;
    }

    synchronized void record(String owner, String origin, String username, String password, String subaccount) {
        String cleanUsername = limited(username == null ? "" : username.trim(), 512);
        String cleanSubaccount = limited(subaccount == null ? "" : subaccount.trim(), 512);
        String cleanPassword = limited(password == null ? "" : password, 4096);
        if (owner == null || owner.isEmpty() || origin == null || origin.isEmpty()
            || cleanPassword.isEmpty() || cleanUsername.isEmpty() && cleanSubaccount.isEmpty()) return;
        List<Entry> values = list(owner);
        long now = System.currentTimeMillis();
        values.removeIf(entry -> entry.origin.equals(origin) && entry.username.equals(cleanUsername)
            && entry.subaccount.equals(cleanSubaccount) && entry.password.equals(cleanPassword));
        values.add(0, new Entry(UUID.randomUUID().toString(), origin, cleanUsername, cleanPassword,
            cleanSubaccount, now));
        if (values.size() > RECORD_LIMIT) values = new ArrayList<>(values.subList(0, RECORD_LIMIT));
        write(owner, values);
    }

    synchronized void delete(String owner, String id) {
        if (id == null || id.isEmpty()) return;
        List<Entry> values = list(owner);
        if (values.removeIf(entry -> entry.id.equals(id))) write(owner, values);
    }

    synchronized void clear(String owner) {
        if (!store.edit().remove(RECORD_PREFIX + owner).commit())
            throw new IllegalStateException("账号密码记录清空失败，请重试");
    }

    private void write(String owner, List<Entry> values) throws IllegalStateException {
        JSONArray result = new JSONArray();
        try {
            for (Entry entry : values) {
                JSONObject value = new JSONObject();
                value.put("id", entry.id);
                value.put("origin", entry.origin);
                value.put("username", entry.username);
                value.put("password", entry.password);
                value.put("subaccount", entry.subaccount);
                value.put("updatedAtMs", Math.max(1L, entry.updatedAtMs));
                result.put(value);
            }
            String encrypted = encrypt(owner, result.toString());
            if (!store.edit().putString(RECORD_PREFIX + owner, encrypted).commit())
                throw new IllegalStateException("账号密码记录保存失败，请重试");
        } catch (IllegalStateException failure) {
            throw failure;
        } catch (Exception failure) {
            throw new IllegalStateException("账号密码记录加密失败，请重试", failure);
        }
    }

    private static SecretKey key() throws Exception {
        KeyStore keyStore = KeyStore.getInstance("AndroidKeyStore");
        keyStore.load(null);
        if (keyStore.containsAlias(KEY_ALIAS)) return (SecretKey) keyStore.getKey(KEY_ALIAS, null);
        KeyGenerator generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore");
        generator.init(new KeyGenParameterSpec.Builder(KEY_ALIAS,
            KeyProperties.PURPOSE_ENCRYPT | KeyProperties.PURPOSE_DECRYPT)
            .setKeySize(256)
            .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
            .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
            .build());
        return generator.generateKey();
    }

    private static String encrypt(String origin, String value) throws Exception {
        Cipher cipher = Cipher.getInstance("AES/GCM/NoPadding");
        cipher.init(Cipher.ENCRYPT_MODE, key());
        cipher.updateAAD(origin.getBytes(StandardCharsets.UTF_8));
        byte[] iv = cipher.getIV(), encrypted = cipher.doFinal(value.getBytes(StandardCharsets.UTF_8));
        byte[] combined = new byte[iv.length + encrypted.length];
        System.arraycopy(iv, 0, combined, 0, iv.length);
        System.arraycopy(encrypted, 0, combined, iv.length, encrypted.length);
        return Base64.encodeToString(combined, Base64.NO_WRAP);
    }

    private static String decrypt(String origin, String value) throws Exception {
        byte[] combined = Base64.decode(value, Base64.NO_WRAP);
        if (combined.length <= 12) throw new IllegalArgumentException("Invalid credential payload");
        byte[] iv = new byte[12], encrypted = new byte[combined.length - 12];
        System.arraycopy(combined, 0, iv, 0, iv.length);
        System.arraycopy(combined, iv.length, encrypted, 0, encrypted.length);
        Cipher cipher = Cipher.getInstance("AES/GCM/NoPadding");
        cipher.init(Cipher.DECRYPT_MODE, key(), new GCMParameterSpec(128, iv));
        cipher.updateAAD(origin.getBytes(StandardCharsets.UTF_8));
        return new String(cipher.doFinal(encrypted), StandardCharsets.UTF_8);
    }

    private static String limited(String value, int maxLength) {
        return value.length() <= maxLength ? value : value.substring(0, maxLength);
    }
}
