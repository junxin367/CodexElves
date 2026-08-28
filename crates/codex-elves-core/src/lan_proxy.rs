use std::net::{IpAddr, Ipv4Addr, SocketAddr};

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
    use std::net::SocketAddr;

    use super::{
        LAN_BIND_HOST, LOOPBACK_BIND_HOST, RemoteAccessError, helper_bind_host, is_lan_ipv4,
        remote_request_access,
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
}
