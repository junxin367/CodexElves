use std::num::NonZeroU32;
use std::path::Path;

use anyhow::{Context, ensure};
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

#[derive(Serialize, Deserialize)]
pub(super) struct TunnelToken {
    pub id: String,
    protected: String,
}

pub(super) fn load_access_key(state_dir: &Path) -> anyhow::Result<Option<AccessKey>> {
    let key: Option<AccessKey> = read_json_or_default(&state_dir.join("remote-access-key.json"))?;
    if let Some(key) = &key {
        ensure!(
            uuid::Uuid::parse_str(&key.id).is_ok()
                && key.salt.len() == 64
                && base64::engine::general_purpose::STANDARD
                    .decode(&key.hash)
                    .is_ok_and(|hash| hash.len() == 32),
            "访问密钥文件损坏，已拒绝授权"
        );
    }
    Ok(key)
}

pub fn configure_access_key(state_dir: &Path, value: &str) -> anyhow::Result<()> {
    let key = value.trim();
    ensure!(
        (32..=256).contains(&key.len()) && key.bytes().all(|byte| byte.is_ascii_graphic()),
        "访问密钥需为 32–256 个非空格 ASCII 字符，建议使用随机生成的密钥"
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
    };
    std::fs::create_dir_all(state_dir)?;
    crate::settings::atomic_write(
        &state_dir.join("remote-access-key.json"),
        &serde_json::to_vec(&record)?,
    )
}

pub(super) fn load_tunnel_token(state_dir: &Path) -> anyhow::Result<Option<TunnelToken>> {
    let record: Option<TunnelToken> =
        read_json_or_default(&state_dir.join("remote-access-secrets/cloudflare-tunnel.json"))?;
    if let Some(record) = &record {
        ensure!(
            uuid::Uuid::parse_str(&record.id).is_ok() && !record.protected.is_empty(),
            "Cloudflare 隧道令牌文件损坏，请重新设置"
        );
    }
    Ok(record)
}

impl TunnelToken {
    pub fn reveal(&self) -> anyhow::Result<String> {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&self.protected)
            .context("Cloudflare 隧道令牌文件损坏")?;
        String::from_utf8(protect_token(&bytes, false)?)
            .context("Cloudflare 隧道令牌无法读取，请重新设置")
    }
}

pub fn configure_tunnel_token(state_dir: &Path, token: &str) -> anyhow::Result<()> {
    let token = token.trim();
    ensure!(
        (20..=8192).contains(&token.len())
            && token.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/' | b'=' | b'-' | b'_')
            }),
        "请只粘贴 Cloudflare 隧道令牌，不要粘贴完整安装命令"
    );
    let directory = state_dir.join("remote-access-secrets");
    std::fs::create_dir_all(&directory)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700))?;
    }
    let record = TunnelToken {
        id: uuid::Uuid::new_v4().to_string(),
        protected: base64::engine::general_purpose::STANDARD
            .encode(protect_token(token.as_bytes(), true)?),
    };
    crate::settings::atomic_write(
        &directory.join("cloudflare-tunnel.json"),
        &serde_json::to_vec(&record)?,
    )
}

pub fn access_credentials_status(state_dir: &Path) -> anyhow::Result<Value> {
    Ok(json!({
        "status":"ok",
        "accessKeyConfigured":load_access_key(state_dir)?.is_some(),
        "tunnelTokenConfigured":load_tunnel_token(state_dir)?.is_some()
    }))
}

#[cfg(windows)]
fn protect_token(bytes: &[u8], encrypt: bool) -> anyhow::Result<Vec<u8>> {
    use windows::Win32::Foundation::{HLOCAL, LocalFree};
    use windows::Win32::Security::Cryptography::{
        CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
    };
    let input = CRYPT_INTEGER_BLOB {
        cbData: bytes.len().try_into()?,
        pbData: bytes.as_ptr().cast_mut(),
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    unsafe {
        if encrypt {
            CryptProtectData(
                &input,
                windows::core::PCWSTR::null(),
                None,
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        } else {
            CryptUnprotectData(
                &input,
                None,
                None,
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        }
        .context("当前 Windows 用户无法保护或读取 Cloudflare 隧道令牌")?;
        let result = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        let _ = LocalFree(HLOCAL(output.pbData.cast()));
        Ok(result)
    }
}

#[cfg(not(windows))]
fn protect_token(bytes: &[u8], _encrypt: bool) -> anyhow::Result<Vec<u8>> {
    Ok(bytes.to_vec())
}
