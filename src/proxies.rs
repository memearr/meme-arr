//! Trusted reverse proxies, like Sonarr's "Trusted Networks" and Jellyfin's
//! "Known Proxies". `X-Forwarded-For` is client-controlled, so it's honored
//! only when the TCP peer is itself a configured proxy -- otherwise anyone
//! could claim any IP and walk around the login rate limit. (Sonarr shipped
//! exactly that bug as CVE-2026-30975.)
//!
//! With no proxies configured (the default) the header is never trusted and
//! the client is the TCP peer.

use std::net::IpAddr;

/// An IP or CIDR range, e.g. `192.168.0.5` or `172.18.0.0/16`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Net {
    addr: IpAddr,
    prefix: u8,
}

impl Net {
    pub fn parse(s: &str) -> Result<Net, String> {
        let s = s.trim();
        let (ip, prefix) = match s.split_once('/') {
            Some((ip, p)) => (ip, Some(p)),
            None => (s, None),
        };
        let addr: IpAddr = ip
            .parse()
            .map_err(|_| format!("'{s}' isn't an IP address or CIDR range"))?;
        let max = if addr.is_ipv4() { 32 } else { 128 };
        let prefix = match prefix {
            None => max,
            Some(p) => p
                .parse::<u8>()
                .ok()
                .filter(|p| *p <= max)
                .ok_or_else(|| format!("'{s}' has an invalid prefix length (0-{max})"))?,
        };
        Ok(Net { addr, prefix })
    }

    pub fn contains(&self, ip: IpAddr) -> bool {
        // An IPv4-mapped IPv6 peer (::ffff:a.b.c.d) is that IPv4 address.
        let ip = match ip {
            IpAddr::V6(v6) => v6.to_ipv4_mapped().map_or(IpAddr::V6(v6), IpAddr::V4),
            v4 => v4,
        };
        match (self.addr, ip) {
            (IpAddr::V4(n), IpAddr::V4(i)) => {
                masked(u32::from(n) as u128, u32::from(i) as u128, self.prefix, 32)
            }
            (IpAddr::V6(n), IpAddr::V6(i)) => {
                masked(u128::from(n), u128::from(i), self.prefix, 128)
            }
            _ => false,
        }
    }
}

fn masked(a: u128, b: u128, prefix: u8, bits: u32) -> bool {
    if prefix == 0 {
        return true;
    }
    let shift = bits - u32::from(prefix);
    (a >> shift) == (b >> shift)
}

/// Parse a comma/space/newline-separated list; blank entries are skipped.
pub fn parse_list(s: &str) -> Result<Vec<Net>, String> {
    s.split(|c: char| c == ',' || c.is_whitespace())
        .filter(|p| !p.is_empty())
        .map(Net::parse)
        .collect()
}

/// The real client IP: the TCP peer, unless the peer is a trusted proxy, in
/// which case walk `X-Forwarded-For` from the right (nearest hop first),
/// skipping further trusted proxies; the first untrusted address is the
/// client. Entries to the left of it are attacker-controlled and ignored.
/// A malformed entry stops the walk -- fail toward the last good address.
pub fn client_ip(peer: IpAddr, forwarded_for: Option<&str>, trusted: &[Net]) -> IpAddr {
    let is_trusted = |ip: IpAddr| trusted.iter().any(|n| n.contains(ip));
    if !is_trusted(peer) {
        return peer;
    }
    let mut current = peer;
    for hop in forwarded_for.unwrap_or("").rsplit(',') {
        match hop.trim().parse::<IpAddr>() {
            Ok(ip) => {
                current = ip;
                if !is_trusted(ip) {
                    break;
                }
            }
            Err(_) => break,
        }
    }
    current
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }
    fn nets(s: &str) -> Vec<Net> {
        parse_list(s).unwrap()
    }

    #[test]
    fn cidr_and_single_ip_matching() {
        let n = Net::parse("172.18.0.0/16").unwrap();
        assert!(n.contains(ip("172.18.5.9")));
        assert!(!n.contains(ip("172.19.0.1")));
        assert!(Net::parse("10.0.0.5").unwrap().contains(ip("10.0.0.5")));
        assert!(!Net::parse("10.0.0.5").unwrap().contains(ip("10.0.0.6")));
        assert!(Net::parse("fd00::/8").unwrap().contains(ip("fd12::1")));
        assert!(!Net::parse("10.0.0.0/8").unwrap().contains(ip("fd12::1")));
        assert!(Net::parse("0.0.0.0/0").unwrap().contains(ip("8.8.8.8")));
    }

    #[test]
    fn ipv4_mapped_ipv6_peers_match_ipv4_rules() {
        assert!(Net::parse("192.168.0.0/24")
            .unwrap()
            .contains(ip("::ffff:192.168.0.7")));
    }

    #[test]
    fn bad_input_is_rejected_with_a_reason() {
        for bad in ["nope", "10.0.0.0/33", "10.0.0.0/x", "::1/129"] {
            assert!(Net::parse(bad).is_err(), "{bad}");
        }
        assert!(parse_list("10.0.0.1, 172.18.0.0/16\n::1").unwrap().len() == 3);
        assert!(parse_list("10.0.0.1, bogus").is_err());
        assert!(parse_list("  ").unwrap().is_empty());
    }

    #[test]
    fn forwarded_for_is_ignored_unless_the_peer_is_a_trusted_proxy() {
        // The whole point: a direct attacker can't pick their own IP.
        assert_eq!(
            client_ip(ip("203.0.113.9"), Some("1.2.3.4"), &nets("10.0.0.1")),
            ip("203.0.113.9")
        );
        assert_eq!(
            client_ip(ip("203.0.113.9"), Some("1.2.3.4"), &[]),
            ip("203.0.113.9")
        );
    }

    #[test]
    fn a_trusted_proxy_vouches_for_the_forwarded_client() {
        assert_eq!(
            client_ip(ip("10.0.0.1"), Some("198.51.100.7"), &nets("10.0.0.1")),
            ip("198.51.100.7")
        );
    }

    #[test]
    fn spoofed_left_entries_are_ignored_by_walking_from_the_right() {
        // Client sent "X-Forwarded-For: 1.2.3.4"; the proxy appended the real peer.
        let got = client_ip(
            ip("10.0.0.1"),
            Some("1.2.3.4, 198.51.100.7"),
            &nets("10.0.0.1"),
        );
        assert_eq!(got, ip("198.51.100.7"));
    }

    #[test]
    fn chained_trusted_proxies_are_skipped() {
        let got = client_ip(
            ip("10.0.0.1"),
            Some("198.51.100.7, 10.0.0.2"),
            &nets("10.0.0.0/24"),
        );
        assert_eq!(got, ip("198.51.100.7"));
    }

    #[test]
    fn missing_or_malformed_headers_fall_back_safely() {
        let t = nets("10.0.0.1");
        assert_eq!(client_ip(ip("10.0.0.1"), None, &t), ip("10.0.0.1"));
        assert_eq!(
            client_ip(ip("10.0.0.1"), Some("garbage"), &t),
            ip("10.0.0.1")
        );
        assert_eq!(
            client_ip(ip("10.0.0.1"), Some("198.51.100.7, garbage"), &t),
            ip("10.0.0.1")
        );
    }
}
