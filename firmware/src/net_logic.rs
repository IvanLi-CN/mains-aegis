use core::fmt::Write as _;

use heapless::String;

use crate::net_types::WifiErrorKind;

pub const RESPONSE_HEAD_CAP: usize = 512;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParsedStaticIpv4Config {
    pub ip: [u8; 4],
    pub gateway: [u8; 4],
    pub prefix_len: u8,
    pub dns: Option<[u8; 4]>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParsedNetEnvConfig {
    pub static_ipv4: Option<ParsedStaticIpv4Config>,
    pub configured_dns: Option<[u8; 4]>,
    pub last_error: Option<WifiErrorKind>,
}

pub fn resolve_net_env_config(
    static_ip: Option<&str>,
    netmask: Option<&str>,
    gateway: Option<&str>,
    dns: Option<&str>,
) -> ParsedNetEnvConfig {
    let configured_dns = dns.and_then(parse_ipv4);
    let has_any_static_field = static_ip.is_some() || netmask.is_some() || gateway.is_some();

    match (static_ip, netmask, gateway) {
        (Some(ip), Some(mask), Some(gateway)) => {
            if let (Some(ip), Some(mask), Some(gateway)) =
                (parse_ipv4(ip), parse_ipv4(mask), parse_ipv4(gateway))
            {
                if let Some(prefix_len) = netmask_to_prefix(mask) {
                    return ParsedNetEnvConfig {
                        static_ipv4: Some(ParsedStaticIpv4Config {
                            ip,
                            gateway,
                            prefix_len,
                            dns: configured_dns,
                        }),
                        configured_dns,
                        last_error: None,
                    };
                }
            }

            ParsedNetEnvConfig {
                static_ipv4: None,
                configured_dns,
                last_error: Some(WifiErrorKind::BadStaticConfig),
            }
        }
        (None, None, None) if !has_any_static_field => ParsedNetEnvConfig {
            static_ipv4: None,
            configured_dns,
            last_error: None,
        },
        _ => ParsedNetEnvConfig {
            static_ipv4: None,
            configured_dns,
            last_error: Some(WifiErrorKind::BadStaticConfig),
        },
    }
}

pub fn select_active_dns(
    configured_dns: Option<[u8; 4]>,
    runtime_dns_servers: &[[u8; 4]],
) -> Option<[u8; 4]> {
    runtime_dns_servers.first().copied().or(configured_dns)
}

pub fn origin_reflection_allowed(origin: &str) -> bool {
    build_http_response_head("503 Service Unavailable", usize::MAX, Some(origin)).is_some()
        && build_sse_response_head(Some(origin)).is_some()
}

pub fn build_http_response_head(
    status: &str,
    body_len: usize,
    origin: Option<&str>,
) -> Option<String<RESPONSE_HEAD_CAP>> {
    let allow_origin = origin.unwrap_or("*");
    let vary = if origin.is_some() {
        "Vary: Origin\r\n"
    } else {
        ""
    };
    let mut head = String::<RESPONSE_HEAD_CAP>::new();
    write!(
        head,
        "HTTP/1.1 {}\r\nContent-Type: application/json; charset=utf-8\r\nAccess-Control-Allow-Origin: {}\r\n{}Access-Control-Allow-Methods: GET, OPTIONS\r\nAccess-Control-Allow-Headers: Accept, Content-Type\r\nAccess-Control-Allow-Private-Network: true\r\nConnection: close\r\nContent-Length: {}\r\n\r\n",
        status,
        allow_origin,
        vary,
        body_len,
    )
    .ok()?;
    Some(head)
}

pub fn build_sse_response_head(origin: Option<&str>) -> Option<String<RESPONSE_HEAD_CAP>> {
    let allow_origin = origin.unwrap_or("*");
    let vary = if origin.is_some() {
        "Vary: Origin\r\n"
    } else {
        ""
    };
    let mut head = String::<RESPONSE_HEAD_CAP>::new();
    write!(
        head,
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nAccess-Control-Allow-Origin: {}\r\n{}Access-Control-Allow-Methods: GET, OPTIONS\r\nAccess-Control-Allow-Headers: Accept, Content-Type\r\nAccess-Control-Allow-Private-Network: true\r\nConnection: keep-alive\r\n\r\n",
        allow_origin,
        vary,
    )
    .ok()?;
    Some(head)
}

pub fn parse_ipv4(input: &str) -> Option<[u8; 4]> {
    let mut octets = [0u8; 4];
    let mut idx = 0usize;
    for part in input.split('.') {
        if idx >= 4 {
            return None;
        }
        octets[idx] = part.parse().ok()?;
        idx += 1;
    }
    if idx == 4 {
        Some(octets)
    } else {
        None
    }
}

pub fn netmask_to_prefix(mask: [u8; 4]) -> Option<u8> {
    let value = u32::from_be_bytes(mask);
    let ones = value.count_ones() as u8;
    let reconstructed = if ones == 0 {
        0
    } else {
        u32::MAX.checked_shl((32 - ones as u32) as u32)?
    };
    if reconstructed == value {
        Some(ones)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{
        build_http_response_head, build_sse_response_head, origin_reflection_allowed, parse_ipv4,
        resolve_net_env_config, select_active_dns,
    };
    use crate::net_types::WifiErrorKind;

    #[test]
    fn dhcp_dns_prefers_runtime_lease_values() {
        assert_eq!(
            select_active_dns(None, &[[192, 168, 31, 1]]),
            Some([192, 168, 31, 1])
        );
        assert_eq!(
            select_active_dns(Some([1, 1, 1, 1]), &[[192, 168, 31, 1]]),
            Some([192, 168, 31, 1])
        );
    }

    #[test]
    fn incomplete_static_ipv4_settings_surface_bad_static_config() {
        let parsed = resolve_net_env_config(Some("192.168.31.15"), None, None, None);
        assert_eq!(parsed.static_ipv4, None);
        assert_eq!(parsed.last_error, Some(WifiErrorKind::BadStaticConfig));
    }

    #[test]
    fn valid_static_ipv4_settings_are_parsed() {
        let parsed = resolve_net_env_config(
            Some("192.168.31.15"),
            Some("255.255.255.0"),
            Some("192.168.31.1"),
            Some("1.1.1.1"),
        );
        assert_eq!(parsed.last_error, None);
        let static_ipv4 = parsed.static_ipv4.expect("static config");
        assert_eq!(static_ipv4.ip, [192, 168, 31, 15]);
        assert_eq!(static_ipv4.gateway, [192, 168, 31, 1]);
        assert_eq!(static_ipv4.prefix_len, 24);
        assert_eq!(static_ipv4.dns, Some([1, 1, 1, 1]));
    }

    #[test]
    fn origin_reflection_rejects_values_that_overflow_response_headers() {
        let long_origin = "https://".to_owned() + &"a".repeat(480);
        assert!(!origin_reflection_allowed(long_origin.as_str()));
        assert!(build_http_response_head("200 OK", 32, Some(long_origin.as_str())).is_none());
        assert!(build_sse_response_head(Some(long_origin.as_str())).is_none());
    }

    #[test]
    fn parse_ipv4_rejects_partial_addresses() {
        assert_eq!(parse_ipv4("192.168.31"), None);
        assert_eq!(parse_ipv4("192.168.31.15"), Some([192, 168, 31, 15]));
    }
}
