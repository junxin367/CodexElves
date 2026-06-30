use std::fs::File;
use std::net::{TcpListener, ToSocketAddrs};
use std::path::{Path, PathBuf};

use fs2::FileExt;

pub const LAUNCHER_GUARD_PORT: u16 = 45220;
pub const MANAGER_GUARD_PORT: u16 = 45219;
pub const DEV_MANAGER_GUARD_PORT: u16 = 45229;
const LAUNCHER_GUARD_PORT_ENV: &str = "CODEX_ELVES_LAUNCHER_GUARD_PORT";
const MANAGER_GUARD_PORT_ENV: &str = "CODEX_ELVES_MANAGER_GUARD_PORT";
const GUARD_PORT_OFFSET_ENV: &str = "CODEX_ELVES_GUARD_PORT_OFFSET";

pub fn launcher_guard_port() -> u16 {
    launcher_guard_port_with(
        std::env::var(LAUNCHER_GUARD_PORT_ENV).ok().as_deref(),
        std::env::var(GUARD_PORT_OFFSET_ENV).ok().as_deref(),
        std::env::var("USERNAME").ok().as_deref(),
        cfg!(windows),
    )
}

fn launcher_guard_port_with(
    env_value: Option<&str>,
    offset_value: Option<&str>,
    username: Option<&str>,
    is_windows: bool,
) -> u16 {
    guard_port_with(
        LAUNCHER_GUARD_PORT,
        env_value,
        offset_value,
        username,
        is_windows,
    )
}

pub fn manager_guard_port() -> u16 {
    manager_guard_port_with(
        cfg!(debug_assertions),
        std::env::var(MANAGER_GUARD_PORT_ENV).ok().as_deref(),
        std::env::var(GUARD_PORT_OFFSET_ENV).ok().as_deref(),
        std::env::var("USERNAME").ok().as_deref(),
        cfg!(windows),
    )
}

fn manager_guard_port_with(
    debug_build: bool,
    env_value: Option<&str>,
    offset_value: Option<&str>,
    username: Option<&str>,
    is_windows: bool,
) -> u16 {
    let base_port = if debug_build {
        DEV_MANAGER_GUARD_PORT
    } else {
        MANAGER_GUARD_PORT
    };
    guard_port_with(base_port, env_value, offset_value, username, is_windows)
}

fn guard_port_with(
    base_port: u16,
    env_value: Option<&str>,
    offset_value: Option<&str>,
    username: Option<&str>,
    is_windows: bool,
) -> u16 {
    if let Some(port) = env_value
        .and_then(|value| value.trim().parse::<u16>().ok())
        .filter(|port| *port > 0)
    {
        return port;
    }

    if let Some(offset) = offset_value.and_then(|value| value.trim().parse::<u16>().ok()) {
        return guard_port_with_offset(base_port, offset);
    }

    guard_port_with_offset(base_port, guard_port_auto_offset(username, is_windows))
}

fn guard_port_with_offset(base_port: u16, offset: u16) -> u16 {
    base_port
        .checked_add(offset)
        .filter(|port| *port > base_port)
        .unwrap_or(base_port)
}

fn guard_port_auto_offset(username: Option<&str>, is_windows: bool) -> u16 {
    if !is_windows {
        return 0;
    }
    username
        .map(|user| {
            user.bytes()
                .fold(0u16, |acc, byte| acc.wrapping_add(byte as u16))
                % 1000
        })
        .unwrap_or(0)
}

pub fn select_platform_loopback_port(requested: u16) -> u16 {
    select_platform_loopback_port_with(
        requested,
        cfg!(windows),
        can_bind_loopback_port,
        find_available_loopback_port,
    )
}

pub fn select_packaged_codex_debug_port(requested: u16) -> u16 {
    select_packaged_codex_debug_port_with(
        requested,
        cfg!(windows),
        can_bind_loopback_port,
        find_available_loopback_port,
    )
}

pub fn select_packaged_codex_debug_port_with(
    requested: u16,
    is_windows: bool,
    can_bind: impl Fn(u16) -> bool,
    find_available: impl Fn() -> u16,
) -> u16 {
    select_platform_loopback_port_with(requested, is_windows, can_bind, find_available)
}

pub fn select_platform_loopback_port_with(
    requested: u16,
    is_windows: bool,
    can_bind: impl Fn(u16) -> bool,
    find_available: impl Fn() -> u16,
) -> u16 {
    if !is_windows || can_bind(requested) {
        requested
    } else {
        find_available()
    }
}

pub fn can_bind_loopback_port(port: u16) -> bool {
    if port == 0 {
        return true;
    }
    TcpListener::bind(("127.0.0.1", port)).is_ok()
}

pub fn find_available_loopback_port() -> u16 {
    TcpListener::bind(("127.0.0.1", 0))
        .and_then(|listener| listener.local_addr())
        .map(|address| address.port())
        .unwrap_or(0)
}

pub fn can_connect_loopback_port(port: u16) -> bool {
    ("127.0.0.1", port)
        .to_socket_addrs()
        .ok()
        .and_then(|mut addresses| addresses.next())
        .and_then(|address| {
            std::net::TcpStream::connect_timeout(&address, std::time::Duration::from_millis(200))
                .ok()
        })
        .is_some()
}

pub fn acquire_loopback_port_guard(port: u16) -> std::io::Result<TcpListener> {
    TcpListener::bind(("127.0.0.1", port))
}

#[derive(Debug)]
pub struct LoopbackPortGuard {
    _lock_file: Option<File>,
    lock_path: Option<PathBuf>,
    _listener: Option<TcpListener>,
    using_fallback_lock: bool,
}

impl LoopbackPortGuard {
    pub fn listener(listener: TcpListener) -> Self {
        Self {
            _lock_file: None,
            lock_path: None,
            _listener: Some(listener),
            using_fallback_lock: false,
        }
    }

    fn locked_listener(file: File, path: PathBuf, listener: TcpListener) -> Self {
        Self {
            _lock_file: Some(file),
            lock_path: Some(path),
            _listener: Some(listener),
            using_fallback_lock: false,
        }
    }

    fn fallback_lock(file: File, path: PathBuf) -> Self {
        Self {
            _lock_file: Some(file),
            lock_path: Some(path),
            _listener: None,
            using_fallback_lock: true,
        }
    }

    pub fn fallback_path(&self) -> Option<&Path> {
        self.using_fallback_lock
            .then_some(())
            .and_then(|_| self.lock_path.as_deref())
    }

    pub fn try_clone_listener(&self) -> std::io::Result<Option<TcpListener>> {
        self._listener
            .as_ref()
            .map(TcpListener::try_clone)
            .transpose()
    }
}

pub fn acquire_resilient_loopback_port_guard(port: u16) -> std::io::Result<LoopbackPortGuard> {
    acquire_resilient_loopback_port_guard_at(port, &crate::paths::default_app_state_dir())
}

fn acquire_resilient_loopback_port_guard_at(
    port: u16,
    state_dir: &Path,
) -> std::io::Result<LoopbackPortGuard> {
    acquire_resilient_loopback_port_guard_with(
        port,
        state_dir,
        acquire_loopback_port_guard,
        can_connect_loopback_port,
    )
}

fn acquire_resilient_loopback_port_guard_with(
    port: u16,
    state_dir: &Path,
    bind: impl Fn(u16) -> std::io::Result<TcpListener>,
    can_connect: impl Fn(u16) -> bool,
) -> std::io::Result<LoopbackPortGuard> {
    if port == 0 {
        return bind(port).map(LoopbackPortGuard::listener);
    }

    let (file, path) = acquire_lock_guard(port, state_dir)?;
    match bind(port) {
        Ok(listener) => Ok(LoopbackPortGuard::locked_listener(file, path, listener)),
        Err(error) if error.kind() == std::io::ErrorKind::AddrInUse && can_connect(port) => {
            Err(error)
        }
        Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => {
            Ok(LoopbackPortGuard::fallback_lock(file, path))
        }
        Err(error) => Err(error),
    }
}

fn acquire_lock_guard(port: u16, state_dir: &Path) -> std::io::Result<(File, PathBuf)> {
    let dir = state_dir.join("locks");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("loopback-port-{port}.lock"));
    let file = File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)?;
    file.try_lock_exclusive().map_err(normalize_lock_error)?;
    Ok((file, path))
}

fn normalize_lock_error(error: std::io::Error) -> std::io::Error {
    match error.raw_os_error() {
        Some(33) => std::io::Error::new(
            std::io::ErrorKind::WouldBlock,
            "loopback port guard lock is already held",
        ),
        _ => error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resilient_guard_holds_lock_and_listener_when_requested_port_is_available() {
        let temp = tempfile::tempdir().unwrap();
        let port = find_available_loopback_port();

        let guard = acquire_resilient_loopback_port_guard_at(port, temp.path()).unwrap();

        assert!(guard.lock_path.is_some());
        assert!(guard._listener.is_some());
        assert!(guard.fallback_path().is_none());
    }

    #[test]
    fn resilient_guard_reports_lock_conflict_when_instance_lock_is_held() {
        let temp = tempfile::tempdir().unwrap();
        let port = find_available_loopback_port();
        let _guard = acquire_resilient_loopback_port_guard_at(port, temp.path()).unwrap();

        let second = acquire_resilient_loopback_port_guard_at(port, temp.path()).unwrap_err();

        assert_eq!(second.kind(), std::io::ErrorKind::WouldBlock);
    }

    #[test]
    fn resilient_guard_reports_conflict_when_requested_port_is_connectable() {
        let temp = tempfile::tempdir().unwrap();
        let error = acquire_resilient_loopback_port_guard_with(
            45219,
            temp.path(),
            |_| {
                Err(std::io::Error::new(
                    std::io::ErrorKind::AddrInUse,
                    "port busy",
                ))
            },
            |_| true,
        )
        .unwrap_err();

        assert_eq!(error.kind(), std::io::ErrorKind::AddrInUse);
    }

    #[test]
    fn manager_guard_port_uses_separate_default_for_debug_builds() {
        assert_eq!(
            manager_guard_port_with(false, None, None, None, false),
            MANAGER_GUARD_PORT
        );
        assert_eq!(
            manager_guard_port_with(true, None, None, None, false),
            DEV_MANAGER_GUARD_PORT
        );
    }

    #[test]
    fn manager_guard_port_allows_environment_override() {
        assert_eq!(
            manager_guard_port_with(true, Some("45299"), None, None, false),
            45299
        );
        assert_eq!(
            manager_guard_port_with(false, Some("45299"), None, None, false),
            45299
        );
        assert_eq!(
            manager_guard_port_with(true, Some("not-a-port"), None, None, false),
            DEV_MANAGER_GUARD_PORT
        );
        assert_eq!(
            manager_guard_port_with(false, Some("0"), None, None, false),
            MANAGER_GUARD_PORT
        );
    }

    #[test]
    fn launcher_guard_port_allows_environment_override() {
        assert_eq!(
            launcher_guard_port_with(Some("45298"), None, Some("alice"), true),
            45298
        );
        assert_eq!(
            launcher_guard_port_with(Some("not-a-port"), None, None, false),
            LAUNCHER_GUARD_PORT
        );
    }

    #[test]
    fn guard_ports_honor_shared_offset_without_collapsing_to_same_port() {
        assert_eq!(
            launcher_guard_port_with(None, Some("50"), Some("alice"), true),
            LAUNCHER_GUARD_PORT + 50
        );
        assert_eq!(
            manager_guard_port_with(false, None, Some("50"), Some("alice"), true),
            MANAGER_GUARD_PORT + 50
        );
    }

    #[test]
    fn guard_ports_ignore_overflowing_shared_offset() {
        assert_eq!(
            launcher_guard_port_with(None, Some("65535"), Some("alice"), true),
            LAUNCHER_GUARD_PORT
        );
        assert_eq!(
            manager_guard_port_with(false, None, Some("65535"), Some("alice"), true),
            MANAGER_GUARD_PORT
        );
    }

    #[test]
    fn guard_ports_use_username_offset_only_on_windows() {
        let windows_port = launcher_guard_port_with(None, None, Some("alice"), true);
        assert!(windows_port >= LAUNCHER_GUARD_PORT);
        assert!(windows_port < LAUNCHER_GUARD_PORT + 1000);
        assert_eq!(
            launcher_guard_port_with(None, None, Some("alice"), false),
            LAUNCHER_GUARD_PORT
        );
    }

    #[test]
    fn resilient_guard_uses_lock_fallback_when_requested_port_is_not_connectable() {
        let temp = tempfile::tempdir().unwrap();
        let guard = acquire_resilient_loopback_port_guard_with(
            45219,
            temp.path(),
            |_| {
                Err(std::io::Error::new(
                    std::io::ErrorKind::AddrInUse,
                    "stale port",
                ))
            },
            |_| false,
        )
        .unwrap();

        assert!(guard._listener.is_none());
        assert!(guard.fallback_path().is_some());

        let second = acquire_resilient_loopback_port_guard_with(
            45219,
            temp.path(),
            |_| {
                Err(std::io::Error::new(
                    std::io::ErrorKind::AddrInUse,
                    "stale port",
                ))
            },
            |_| false,
        )
        .unwrap_err();
        assert_eq!(second.kind(), std::io::ErrorKind::WouldBlock);
    }
}
