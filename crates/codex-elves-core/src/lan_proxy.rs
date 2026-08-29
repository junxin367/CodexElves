use std::collections::HashSet;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};

pub(crate) const LOOPBACK_BIND_HOST: &str = "127.0.0.1";
pub(crate) const LAN_BIND_HOST: &str = "0.0.0.0";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RemoteAccessError {
    Disabled,
    ForbiddenNetwork,
    ForbiddenRoute,
}

pub(crate) fn helper_bind_host(lan_proxy_enabled: bool) -> &'static str {
    if lan_proxy_enabled {
        LAN_BIND_HOST
    } else {
        LOOPBACK_BIND_HOST
    }
}

pub fn lan_ipv4_addresses() -> Vec<Ipv4Addr> {
    let preferred = preferred_lan_ipv4();
    let addresses = if_addrs::get_if_addrs()
        .map(|interfaces| {
            interfaces
                .into_iter()
                .map(|interface| interface.ip())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    collect_lan_ipv4_addresses(addresses, preferred)
}

fn preferred_lan_ipv4() -> Option<Ipv4Addr> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    socket.connect((Ipv4Addr::new(1, 1, 1, 1), 80)).ok()?;
    let IpAddr::V4(ip) = socket.local_addr().ok()?.ip() else {
        return None;
    };
    is_lan_ipv4(ip).then_some(ip)
}

fn collect_lan_ipv4_addresses(
    addresses: impl IntoIterator<Item = IpAddr>,
    preferred: Option<Ipv4Addr>,
) -> Vec<Ipv4Addr> {
    let mut seen = HashSet::new();
    let mut collected = addresses
        .into_iter()
        .filter_map(|address| match address {
            IpAddr::V4(ip) if is_lan_ipv4(ip) && seen.insert(ip) => Some(ip),
            _ => None,
        })
        .collect::<Vec<_>>();

    if let Some(preferred) = preferred.filter(|ip| is_lan_ipv4(*ip)) {
        if let Some(index) = collected.iter().position(|ip| *ip == preferred) {
            collected.remove(index);
        }
        collected.insert(0, preferred);
    }

    collected
}

fn is_lan_ipv4(ip: Ipv4Addr) -> bool {
    let octets = ip.octets();
    ip.is_private() || ip.is_link_local() || (octets[0] == 100 && (64..=127).contains(&octets[1]))
}

pub(crate) fn remote_request_access(
    remote_addr: Option<SocketAddr>,
    lan_proxy_enabled: bool,
    protocol_proxy_enabled: bool,
    method: &str,
    path: &str,
) -> Result<(), RemoteAccessError> {
    let Some(remote_addr) = remote_addr else {
        return Ok(());
    };
    if remote_addr.ip().is_loopback() {
        return Ok(());
    }
    if !lan_proxy_enabled || !protocol_proxy_enabled {
        return Err(RemoteAccessError::Disabled);
    }
    let IpAddr::V4(remote_ip) = remote_addr.ip() else {
        return Err(RemoteAccessError::ForbiddenNetwork);
    };
    if !is_lan_ipv4(remote_ip) {
        return Err(RemoteAccessError::ForbiddenNetwork);
    }
    if !is_allowed_proxy_request(method, path) {
        return Err(RemoteAccessError::ForbiddenRoute);
    }
    Ok(())
}

fn is_allowed_proxy_request(method: &str, path: &str) -> bool {
    if method.eq_ignore_ascii_case("OPTIONS") {
        return is_proxy_path(path);
    }
    if crate::protocol_proxy::is_responses_proxy_path(path) {
        return method.eq_ignore_ascii_case("POST");
    }
    if crate::protocol_proxy::is_chat_completions_proxy_path(path) {
        return method.eq_ignore_ascii_case("POST");
    }
    crate::protocol_proxy::is_models_proxy_path(path) && method.eq_ignore_ascii_case("GET")
}

fn is_proxy_path(path: &str) -> bool {
    crate::protocol_proxy::is_responses_proxy_path(path)
        || crate::protocol_proxy::is_chat_completions_proxy_path(path)
        || crate::protocol_proxy::is_models_proxy_path(path)
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    use super::{
        LAN_BIND_HOST, LOOPBACK_BIND_HOST, RemoteAccessError, collect_lan_ipv4_addresses,
        helper_bind_host, is_lan_ipv4, remote_request_access,
    };

    #[test]
    fn chooses_listener_host_from_lan_setting() {
        assert_eq!(helper_bind_host(false), LOOPBACK_BIND_HOST);
        assert_eq!(helper_bind_host(true), LAN_BIND_HOST);
    }

    #[test]
    fn recognizes_private_link_local_and_shared_ipv4_ranges() {
        assert!(is_lan_ipv4("10.0.0.2".parse().unwrap()));
        assert!(is_lan_ipv4("172.16.5.4".parse().unwrap()));
        assert!(is_lan_ipv4("192.168.1.5".parse().unwrap()));
        assert!(is_lan_ipv4("169.254.4.5".parse().unwrap()));
        assert!(is_lan_ipv4("100.64.4.5".parse().unwrap()));
        assert!(!is_lan_ipv4("127.0.0.1".parse().unwrap()));
        assert!(!is_lan_ipv4("8.8.8.8".parse().unwrap()));
    }

    #[test]
    fn loopback_requests_keep_existing_unrestricted_helper_access() {
        let result = remote_request_access(
            Some(SocketAddr::from(([127, 0, 0, 1], 50000))),
            false,
            false,
            "POST",
            "/backend/status",
        );

        assert_eq!(result, Ok(()));
    }

    #[test]
    fn remote_requests_require_enabled_proxy_and_allowed_route() {
        let remote = Some(SocketAddr::from(([192, 168, 1, 25], 50000)));

        assert_eq!(
            remote_request_access(remote, false, true, "POST", "/v1/responses"),
            Err(RemoteAccessError::Disabled)
        );
        assert_eq!(
            remote_request_access(remote, true, true, "POST", "/diagnostics/log"),
            Err(RemoteAccessError::ForbiddenRoute)
        );
        assert_eq!(
            remote_request_access(remote, true, true, "POST", "/v1/responses"),
            Ok(())
        );
    }

    #[test]
    fn remote_requests_reject_non_lan_source_addresses() {
        let remote = Some(SocketAddr::from(([203, 0, 113, 25], 50000)));

        assert_eq!(
            remote_request_access(remote, true, true, "POST", "/v1/responses"),
            Err(RemoteAccessError::ForbiddenNetwork)
        );
    }

    #[test]
    fn remote_responses_requests_reject_unsupported_get_method() {
        let remote = Some(SocketAddr::from(([192, 168, 1, 25], 50000)));

        assert_eq!(
            remote_request_access(remote, true, true, "GET", "/v1/responses"),
            Err(RemoteAccessError::ForbiddenRoute)
        );
    }

    #[test]
    fn remote_preflight_only_requires_an_allowed_proxy_path() {
        let remote = Some(SocketAddr::from(([192, 168, 1, 25], 50000)));

        assert_eq!(
            remote_request_access(remote, true, true, "OPTIONS", "/v1/chat/completions"),
            Ok(())
        );
    }

    #[test]
    fn collects_unique_lan_ipv4_addresses_with_preferred_address_first() {
        let preferred = Ipv4Addr::new(192, 168, 1, 20);
        let addresses = collect_lan_ipv4_addresses(
            [
                IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
                IpAddr::V4(Ipv4Addr::new(10, 0, 0, 4)),
                IpAddr::V4(preferred),
                IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)),
                IpAddr::V4(Ipv4Addr::new(10, 0, 0, 4)),
                IpAddr::V6("fe80::1".parse().unwrap()),
            ],
            Some(preferred),
        );

        assert_eq!(addresses, vec![preferred, Ipv4Addr::new(10, 0, 0, 4)]);
    }
}
