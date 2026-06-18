use std::fs;
use std::net::Ipv6Addr;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum RemoteSshError {
    #[error("{0}")]
    Validation(&'static str),
    #[error("Cannot read Codex remote connection state")]
    StateRead(#[source] std::io::Error),
    #[error("Cannot parse Codex remote connection state")]
    StateParse(#[source] serde_json::Error),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SshTarget {
    pub user: String,
    pub host: String,
    pub port: Option<u16>,
}

pub fn codex_global_state_path() -> PathBuf {
    crate::codex_home::default_codex_home_dir().join(".codex-global-state.json")
}

fn string_value(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(value)) => value.trim().to_string(),
        Some(Value::Number(value)) => value.to_string(),
        _ => String::new(),
    }
}

pub fn split_ssh_authority(value: &str) -> Result<(String, String, Option<u16>), RemoteSshError> {
    let mut authority = value.trim();
    if authority.is_empty() {
        return Ok((String::new(), String::new(), None));
    }
    let mut user = "";
    if let Some(index) = authority.rfind('@') {
        user = &authority[..index];
        authority = &authority[index + 1..];
    }

    if authority.starts_with('[') {
        if let Some(close_index) = authority.find(']') {
            let host = authority[..=close_index].trim().to_string();
            let suffix = &authority[close_index + 1..];
            let port = if let Some(raw_port) = suffix.strip_prefix(':') {
                parse_port_str(raw_port)?
            } else {
                None
            };
            return Ok((user.trim().to_string(), host, port));
        }
        return Ok((user.trim().to_string(), authority.trim().to_string(), None));
    }

    if authority.matches(':').count() == 1 {
        let (host, raw_port) = authority.rsplit_once(':').unwrap_or((authority, ""));
        if raw_port.chars().all(|ch| ch.is_ascii_digit()) && !raw_port.is_empty() {
            return Ok((
                user.trim().to_string(),
                host.trim().to_string(),
                parse_port_str(raw_port)?,
            ));
        }
    }
    Ok((user.trim().to_string(), authority.trim().to_string(), None))
}

fn parse_port_value(value: Option<&Value>) -> Result<Option<u16>, RemoteSshError> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) if value.trim().is_empty() => Ok(None),
        Some(Value::String(value)) => parse_port_str(value.trim()),
        Some(Value::Number(value)) => {
            let port = value
                .as_u64()
                .ok_or(RemoteSshError::Validation("Invalid SSH port"))?;
            u16::try_from(port)
                .ok()
                .filter(|port| *port >= 1)
                .ok_or(RemoteSshError::Validation("Invalid SSH port"))
                .map(Some)
        }
        _ => Err(RemoteSshError::Validation("Invalid SSH port")),
    }
}

fn parse_port_str(value: &str) -> Result<Option<u16>, RemoteSshError> {
    if value.is_empty() {
        return Ok(None);
    }
    if !value.chars().all(|ch| ch.is_ascii_digit()) {
        return Err(RemoteSshError::Validation("Invalid SSH port"));
    }
    let port: u16 = value
        .parse()
        .map_err(|_| RemoteSshError::Validation("Invalid SSH port"))?;
    if port == 0 {
        return Err(RemoteSshError::Validation("Invalid SSH port"));
    }
    Ok(Some(port))
}

pub fn validate_ssh_host(host: &str) -> Result<String, RemoteSshError> {
    let host = host.trim();
    if host.is_empty() {
        return Err(RemoteSshError::Validation(
            "Cannot determine remote SSH host for this file",
        ));
    }
    if host
        .chars()
        .any(|ch| ch.is_control() || ch.is_whitespace() || matches!(ch, '/' | '?' | '#' | '@'))
    {
        return Err(RemoteSshError::Validation("Invalid SSH host"));
    }
    if host.starts_with('[') || host.ends_with(']') {
        if !(host.starts_with('[') && host.ends_with(']')) {
            return Err(RemoteSshError::Validation("Invalid SSH host"));
        }
        host[1..host.len() - 1]
            .parse::<Ipv6Addr>()
            .map_err(|_| RemoteSshError::Validation("Invalid SSH host"))?;
        return Ok(host.to_string());
    }
    if host.contains('[') || host.contains(']') {
        return Err(RemoteSshError::Validation("Invalid SSH host"));
    }
    Ok(host.to_string())
}

pub fn target_from_managed_remote_connection(
    connection: &serde_json::Map<String, Value>,
) -> Result<SshTarget, RemoteSshError> {
    let ssh_host = string_value(connection.get("sshHost"))
        .or_else_nonempty(|| string_value(connection.get("hostname")));
    let ssh_alias = string_value(connection.get("sshAlias"))
        .or_else_nonempty(|| string_value(connection.get("alias")));
    let (authority_user, authority_host, authority_port) = split_ssh_authority(&ssh_host)?;
    let host = authority_host.or_else_nonempty(|| ssh_alias.clone());
    let user = string_value(connection.get("sshUser"))
        .or_else_nonempty(|| string_value(connection.get("user")))
        .or_else_nonempty(|| authority_user.clone());
    let port = match connection.get("sshPort") {
        Some(Value::Null) | None => authority_port,
        Some(Value::String(value)) if value.trim().is_empty() => authority_port,
        value => parse_port_value(value)?,
    };
    Ok(SshTarget {
        user,
        host: validate_ssh_host(&host)?,
        port,
    })
}

pub fn resolve_ssh_target_from_global_state(
    state: &Value,
    host_id: &str,
) -> Result<SshTarget, RemoteSshError> {
    if host_id.is_empty() {
        return Err(RemoteSshError::Validation("Remote host id is required"));
    }
    let connections = state
        .get("codex-managed-remote-connections")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for connection in connections {
        let Some(connection) = connection.as_object() else {
            continue;
        };
        if string_value(connection.get("hostId")) != host_id {
            continue;
        }
        return target_from_managed_remote_connection(connection);
    }
    Err(RemoteSshError::Validation(
        "Cannot resolve remote SSH host for this file",
    ))
}

pub fn resolve_ssh_target_for_host_id(
    host_id: &str,
    state_path: Option<&Path>,
) -> Result<SshTarget, RemoteSshError> {
    if host_id.is_empty() {
        return Err(RemoteSshError::Validation("Remote host id is required"));
    }
    let path = state_path
        .map(Path::to_path_buf)
        .unwrap_or_else(codex_global_state_path);
    let data = fs::read_to_string(path).map_err(RemoteSshError::StateRead)?;
    let state: Value = serde_json::from_str(&data).map_err(RemoteSshError::StateParse)?;
    resolve_ssh_target_from_global_state(&state, host_id)
}

trait NonEmptyStringExt {
    fn or_else_nonempty<F>(self, fallback: F) -> String
    where
        F: FnOnce() -> String;
}

impl NonEmptyStringExt for String {
    fn or_else_nonempty<F>(self, fallback: F) -> String
    where
        F: FnOnce() -> String,
    {
        if self.is_empty() { fallback() } else { self }
    }
}
