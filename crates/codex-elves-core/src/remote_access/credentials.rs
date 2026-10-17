use std::fs::{File, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use anyhow::ensure;
use base64::Engine;
use fs2::FileExt;
use ring::pbkdf2;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{random_token, read_json_or_default};

const KEY_ITERATIONS: NonZeroU32 = NonZeroU32::new(600_000).unwrap();
const MAX_LOGIN_FAILURES: u8 = 4;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct AccessKey {
    pub id: String,
    salt: String,
    hash: String,
    #[serde(default)]
    value: String,
    #[serde(
        default,
        deserialize_with = "deserialize_present_login_state",
        skip_serializing_if = "Option::is_none"
    )]
    login_failures: Option<Value>,
    #[serde(
        default,
        deserialize_with = "deserialize_present_login_state",
        skip_serializing_if = "Option::is_none"
    )]
    login_state_version: Option<Value>,
}

struct VerifiedKeyMaterial {
    id: String,
    salt: String,
    hash: String,
    value: String,
}

fn verify_stored_key_material(key: &AccessKey) -> bool {
    // Cache only the successful KDF result for one exact set of key material.
    // The file and login state must still be read and checked on every request.
    static VERIFIED: OnceLock<Mutex<Option<VerifiedKeyMaterial>>> = OnceLock::new();
    let mut verified = VERIFIED
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if verified.as_ref().is_some_and(|cached| {
        cached.id == key.id
            && cached.salt == key.salt
            && cached.hash == key.hash
            && cached.value == key.value
    }) {
        return true;
    }
    if !key.verify(&key.value) {
        return false;
    }
    *verified = Some(VerifiedKeyMaterial {
        id: key.id.clone(),
        salt: key.salt.clone(),
        hash: key.hash.clone(),
        value: key.value.clone(),
    });
    true
}

fn deserialize_present_login_state<'de, D>(deserializer: D) -> Result<Option<Value>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Value::deserialize(deserializer).map(Some)
}

impl AccessKey {
    fn login_failure_count(&self) -> Option<u64> {
        match (&self.login_state_version, &self.login_failures) {
            (None, None) => Some(0),
            (Some(version), Some(failures)) if version.as_u64() == Some(1) => failures
                .as_u64()
                .filter(|count| *count <= u64::from(MAX_LOGIN_FAILURES)),
            _ => None,
        }
    }

    pub fn login_locked(&self) -> bool {
        self.login_failure_count()
            .is_none_or(|count| count >= u64::from(MAX_LOGIN_FAILURES))
    }

    pub fn verify(&self, key: &str) -> bool {
        let Ok(digest) = base64::engine::general_purpose::STANDARD.decode(&self.hash) else {
            return false;
        };
        pbkdf2::verify(
            pbkdf2::PBKDF2_HMAC_SHA256,
            KEY_ITERATIONS,
            self.salt.as_bytes(),
            key.as_bytes(),
            &digest,
        )
        .is_ok()
    }
}

pub(super) struct AccessKeyStore {
    state_dir: PathBuf,
    lock: File,
}

pub(super) fn lock_access_key_store(state_dir: &Path) -> anyhow::Result<AccessKeyStore> {
    std::fs::create_dir_all(state_dir)?;
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(state_dir.join("remote-access-key.lock"))?;
    FileExt::lock_exclusive(&lock)?;
    Ok(AccessKeyStore {
        state_dir: state_dir.to_path_buf(),
        lock,
    })
}

impl AccessKeyStore {
    pub fn load(&self) -> anyhow::Result<Option<AccessKey>> {
        load_access_key_unlocked(&self.state_dir)
    }

    pub fn login_locked(&mut self, key: Option<&AccessKey>) -> anyhow::Result<bool> {
        // The locked file also holds a durable, short update marker. An interrupted
        // counter write must not turn into a fresh allowance after a service restart.
        // Any nonempty marker, including malformed bytes, blocks new logins.
        Ok(key.is_some_and(AccessKey::login_locked) || self.lock.metadata()?.len() != 0)
    }

    pub fn record_login_result(&mut self, key: &mut AccessKey, valid: bool) -> anyhow::Result<()> {
        ensure!(!self.login_locked(Some(key))?, "访问密钥登录已锁定");
        if valid && key.login_failure_count() == Some(0) {
            return Ok(());
        }
        // Append rather than truncate first: even a partial marker is fail-closed.
        self.lock.seek(SeekFrom::End(0))?;
        self.lock.write_all(b"pending")?;
        self.lock.sync_data()?;
        let mut updated = key.clone();
        updated.login_state_version = Some(json!(1));
        updated.login_failures = Some(json!(if valid {
            0
        } else {
            key.login_failure_count()
                .unwrap_or(u64::from(MAX_LOGIN_FAILURES))
                .saturating_add(1)
                .min(u64::from(MAX_LOGIN_FAILURES))
        }));
        crate::settings::atomic_write(
            &self.state_dir.join("remote-access-key.json"),
            &serde_json::to_vec(&updated)?,
        )?;
        self.lock.set_len(0)?;
        self.lock.sync_data()?;
        *key = updated;
        Ok(())
    }
}

fn load_access_key_unlocked(state_dir: &Path) -> anyhow::Result<Option<AccessKey>> {
    let key: Option<AccessKey> = read_json_or_default(&state_dir.join("remote-access-key.json"))?;
    if let Some(key) = &key {
        ensure!(
            uuid::Uuid::parse_str(&key.id).is_ok()
                && key.salt.len() == 64
                && base64::engine::general_purpose::STANDARD
                    .decode(&key.hash)
                    .is_ok_and(|hash| hash.len() == 32)
                && (key.value.is_empty()
                    || ((8..=256).contains(&key.value.len())
                        && key.value.bytes().all(|byte| byte.is_ascii_graphic())
                        && verify_stored_key_material(key))),
            "访问密钥文件损坏，已拒绝授权"
        );
    }
    Ok(key)
}

#[cfg(test)]
fn load_access_key(state_dir: &Path) -> anyhow::Result<Option<AccessKey>> {
    lock_access_key_store(state_dir)?.load()
}

pub fn configure_access_key(state_dir: &Path, value: &str) -> anyhow::Result<()> {
    let key = value.trim();
    ensure!(
        (8..=256).contains(&key.len()) && key.bytes().all(|byte| byte.is_ascii_graphic()),
        "访问密钥需为 8–256 个非空格 ASCII 字符，建议使用随机生成的密钥"
    );
    let store = lock_access_key_store(state_dir)?;
    // Saving the existing secret is not a rotation and must not unlock it.
    match store.load() {
        Ok(Some(mut current)) if current.verify(key) => {
            if current.value.is_empty() {
                current.value = key.to_string();
                crate::settings::atomic_write(
                    &state_dir.join("remote-access-key.json"),
                    &serde_json::to_vec(&current)?,
                )?;
            }
            return Ok(());
        }
        Err(_) => {
            // Recover readable key material only to reject resetting the same
            // secret when the rest of the credential record is damaged.
            let raw: Option<Value> =
                read_json_or_default(&state_dir.join("remote-access-key.json"))
                    .ok()
                    .flatten();
            if let Some(mut raw) = raw {
                if let Some(object) = raw.as_object_mut() {
                    object.remove("login_failures");
                }
                let same_secret = serde_json::from_value::<AccessKey>(raw)
                    .is_ok_and(|current| current.value == key || current.verify(key));
                ensure!(!same_secret, "登录状态损坏，请重新生成并保存不同的访问密钥");
            }
        }
        _ => {}
    }
    let salt = random_token();
    let mut digest = [0u8; 32];
    pbkdf2::derive(
        pbkdf2::PBKDF2_HMAC_SHA256,
        KEY_ITERATIONS,
        salt.as_bytes(),
        key.as_bytes(),
        &mut digest,
    );
    let record = AccessKey {
        id: uuid::Uuid::new_v4().to_string(),
        salt,
        hash: base64::engine::general_purpose::STANDARD.encode(digest),
        value: key.to_string(),
        login_failures: Some(json!(0)),
        login_state_version: Some(json!(1)),
    };
    crate::settings::atomic_write(
        &state_dir.join("remote-access-key.json"),
        &serde_json::to_vec(&record)?,
    )?;
    // Only a successfully saved, different key may clear an interrupted update.
    store.lock.set_len(0)?;
    store.lock.sync_data()?;
    Ok(())
}

pub fn access_credentials_status(state_dir: &Path) -> anyhow::Result<Value> {
    let mut store = lock_access_key_store(state_dir)?;
    let key = store.load()?;
    let login_locked = store.login_locked(key.as_ref())?;
    let mut status = json!({
        "status":"ok",
        "accessKeyConfigured":key.is_some(),
        "loginLocked":login_locked,
    });
    if let Some(value) = key
        .as_ref()
        .map(|key| key.value.as_str())
        .filter(|value| !value.is_empty())
    {
        status["key"] = json!(value);
    }
    Ok(status)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_state_dir() -> tempfile::TempDir {
        let temp_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../temp");
        std::fs::create_dir_all(&temp_root).unwrap();
        tempfile::Builder::new()
            .prefix("remote-login-credentials-")
            .tempdir_in(temp_root)
            .unwrap()
    }

    #[test]
    fn configured_key_status_returns_saved_plaintext() {
        let state_dir = temp_state_dir();

        configure_access_key(state_dir.path(), "test-key-123").unwrap();
        let status = access_credentials_status(state_dir.path()).unwrap();

        assert_eq!(status["accessKeyConfigured"], true);
        assert_eq!(status["key"], "test-key-123");
        assert!(status.get("hash").is_none());
    }

    #[test]
    fn legacy_hash_only_key_remains_valid_without_plaintext_status() {
        let state_dir = temp_state_dir();
        let path = state_dir.path().join("remote-access-key.json");
        configure_access_key(state_dir.path(), "legacy-key-123").unwrap();
        let mut stored: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        stored.as_object_mut().unwrap().remove("value");
        stored.as_object_mut().unwrap().remove("login_failures");
        stored
            .as_object_mut()
            .unwrap()
            .remove("login_state_version");
        crate::settings::atomic_write(&path, &serde_json::to_vec(&stored).unwrap()).unwrap();

        let key = load_access_key(state_dir.path()).unwrap().unwrap();
        let status = access_credentials_status(state_dir.path()).unwrap();

        assert!(key.verify("legacy-key-123"));
        assert!(!key.login_locked());
        assert_eq!(status["accessKeyConfigured"], true);
        assert!(status.get("key").is_none());

        configure_access_key(state_dir.path(), "legacy-key-123").unwrap();
        let saved: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert!(saved.get("login_failures").is_none());
        assert!(saved.get("login_state_version").is_none());
        let mut store = lock_access_key_store(state_dir.path()).unwrap();
        let mut saved_key = store.load().unwrap().unwrap();
        assert_eq!(saved_key.id, key.id);
        assert!(saved_key.verify("legacy-key-123"));
        store.record_login_result(&mut saved_key, true).unwrap();
        assert!(!saved_key.login_locked());
        assert_eq!(saved_key.value, "legacy-key-123");
    }

    #[test]
    fn saving_existing_hash_only_key_restores_plaintext_without_unlocking_or_rotating() {
        let state_dir = temp_state_dir();
        let path = state_dir.path().join("remote-access-key.json");
        configure_access_key(state_dir.path(), "legacy-key-123").unwrap();
        let original_id = {
            let mut store = lock_access_key_store(state_dir.path()).unwrap();
            let mut key = store.load().unwrap().unwrap();
            for _ in 0..MAX_LOGIN_FAILURES {
                store.record_login_result(&mut key, false).unwrap();
            }
            key.id
        };
        let mut stored: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        stored.as_object_mut().unwrap().remove("value");
        crate::settings::atomic_write(&path, &serde_json::to_vec(&stored).unwrap()).unwrap();

        configure_access_key(state_dir.path(), "legacy-key-123").unwrap();

        let key = load_access_key(state_dir.path()).unwrap().unwrap();
        assert_eq!(key.id, original_id);
        assert_eq!(key.login_failures, Some(json!(MAX_LOGIN_FAILURES)));
        let status = access_credentials_status(state_dir.path()).unwrap();
        assert_eq!(status["key"], "legacy-key-123");
        assert_eq!(status["loginLocked"], true);
    }

    #[test]
    fn corrupt_login_counter_stays_locked_when_saving_same_secret_but_new_secret_recovers() {
        let state_dir = temp_state_dir();
        let path = state_dir.path().join("remote-access-key.json");
        configure_access_key(state_dir.path(), "original-test-key").unwrap();
        let mut stored: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        stored["login_failures"] = json!("invalid");
        crate::settings::atomic_write(&path, &serde_json::to_vec(&stored).unwrap()).unwrap();

        assert!(
            load_access_key(state_dir.path())
                .unwrap()
                .unwrap()
                .login_locked()
        );
        assert_eq!(
            access_credentials_status(state_dir.path()).unwrap()["loginLocked"],
            true
        );
        configure_access_key(state_dir.path(), "original-test-key").unwrap();
        assert_eq!(
            access_credentials_status(state_dir.path()).unwrap()["loginLocked"],
            true
        );
        configure_access_key(state_dir.path(), "replacement-test-key").unwrap();
        assert_eq!(
            access_credentials_status(state_dir.path()).unwrap()["loginLocked"],
            false
        );
    }

    #[test]
    fn modern_login_state_missing_counter_or_version_is_locked() {
        let state_dir = temp_state_dir();
        let path = state_dir.path().join("remote-access-key.json");
        configure_access_key(state_dir.path(), "missing-state-test-key").unwrap();
        let stored: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        for missing in ["login_failures", "login_state_version"] {
            let mut corrupted = stored.clone();
            corrupted.as_object_mut().unwrap().remove(missing);
            crate::settings::atomic_write(&path, &serde_json::to_vec(&corrupted).unwrap()).unwrap();
            assert_eq!(
                access_credentials_status(state_dir.path()).unwrap()["loginLocked"],
                true
            );
            configure_access_key(state_dir.path(), "missing-state-test-key").unwrap();
            assert_eq!(
                access_credentials_status(state_dir.path()).unwrap()["loginLocked"],
                true
            );
        }
    }

    #[test]
    fn explicit_null_login_state_stays_locked_until_new_secret_is_saved() {
        let state_dir = temp_state_dir();
        let path = state_dir.path().join("remote-access-key.json");
        configure_access_key(state_dir.path(), "null-state-test-key").unwrap();
        let mut stored: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        stored["login_failures"] = Value::Null;
        stored["login_state_version"] = Value::Null;
        crate::settings::atomic_write(&path, &serde_json::to_vec(&stored).unwrap()).unwrap();

        assert!(
            load_access_key(state_dir.path())
                .unwrap()
                .unwrap()
                .login_locked()
        );
        assert_eq!(
            access_credentials_status(state_dir.path()).unwrap()["loginLocked"],
            true
        );
        configure_access_key(state_dir.path(), "null-state-test-key").unwrap();
        assert_eq!(
            access_credentials_status(state_dir.path()).unwrap()["loginLocked"],
            true
        );
        configure_access_key(state_dir.path(), "replacement-null-state-key").unwrap();
        assert_eq!(
            access_credentials_status(state_dir.path()).unwrap()["loginLocked"],
            false
        );
    }
}
