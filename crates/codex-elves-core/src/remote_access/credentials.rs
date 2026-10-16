use std::num::NonZeroU32;
use std::path::Path;

use anyhow::ensure;
use base64::Engine;
use ring::pbkdf2;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{random_token, read_json_or_default};

const KEY_ITERATIONS: NonZeroU32 = NonZeroU32::new(600_000).unwrap();

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct AccessKey {
    pub id: String,
    salt: String,
    hash: String,
    #[serde(default)]
    value: String,
}

impl AccessKey {
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

pub(super) fn load_access_key(state_dir: &Path) -> anyhow::Result<Option<AccessKey>> {
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
                        && key.verify(&key.value))),
            "访问密钥文件损坏，已拒绝授权"
        );
    }
    Ok(key)
}

pub fn configure_access_key(state_dir: &Path, value: &str) -> anyhow::Result<()> {
    let key = value.trim();
    ensure!(
        (8..=256).contains(&key.len()) && key.bytes().all(|byte| byte.is_ascii_graphic()),
        "访问密钥需为 8–256 个非空格 ASCII 字符，建议使用随机生成的密钥"
    );
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
    };
    std::fs::create_dir_all(state_dir)?;
    crate::settings::atomic_write(
        &state_dir.join("remote-access-key.json"),
        &serde_json::to_vec(&record)?,
    )
}

pub fn access_credentials_status(state_dir: &Path) -> anyhow::Result<Value> {
    let key = load_access_key(state_dir)?;
    let mut status = json!({
        "status":"ok",
        "accessKeyConfigured":key.is_some(),
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

    #[test]
    fn configured_key_status_returns_saved_plaintext() {
        let state_dir = tempfile::tempdir().unwrap();

        configure_access_key(state_dir.path(), "test-key-123").unwrap();
        let status = access_credentials_status(state_dir.path()).unwrap();

        assert_eq!(status["accessKeyConfigured"], true);
        assert_eq!(status["key"], "test-key-123");
        assert!(status.get("hash").is_none());
    }

    #[test]
    fn legacy_hash_only_key_remains_valid_without_plaintext_status() {
        let state_dir = tempfile::tempdir().unwrap();
        let path = state_dir.path().join("remote-access-key.json");
        configure_access_key(state_dir.path(), "legacy-key-123").unwrap();
        let mut stored: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        stored.as_object_mut().unwrap().remove("value");
        crate::settings::atomic_write(&path, &serde_json::to_vec(&stored).unwrap()).unwrap();

        let key = load_access_key(state_dir.path()).unwrap().unwrap();
        let status = access_credentials_status(state_dir.path()).unwrap();

        assert!(key.verify("legacy-key-123"));
        assert_eq!(status["accessKeyConfigured"], true);
        assert!(status.get("key").is_none());
    }
}
