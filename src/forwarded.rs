//! How a request reached us when a reverse proxy may sit in front: the address
//! the browser actually used (scheme, host, any path prefix the proxy added),
//! which decides the login callback URLs, whether cookies are `Secure`, and the
//! page's base path.
//!
//! The `X-Forwarded-*` and `Forwarded` headers are written by whoever sends the
//! request, so they are believed only from a sender that is plausibly a proxy of
//! ours: one on the trusted-proxies list, or one on a private/loopback address
//! (the usual Docker or LAN proxy). A client on the open internet that sends
//! them is ignored. `PUBLIC_URL` overrides all of it for setups where the
//! headers can't be relied on.

use axum::http::{header, HeaderMap};
use std::net::IpAddr;

use crate::proxies::Net;

/// The address people use to reach the app (`PUBLIC_URL`), for setups where
/// the proxy's headers can't be relied on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicUrl {
    pub https: bool,
    pub host: String,
    /// Path part with no trailing slash: "" or e.g. "/memes".
    pub path: String,
}

impl PublicUrl {
    pub fn parse(raw: &str) -> Result<PublicUrl, String> {
        let raw = raw.trim().trim_end_matches('/');
        let (https, rest) = if let Some(r) = raw.strip_prefix("https://") {
            (true, r)
        } else if let Some(r) = raw.strip_prefix("http://") {
            (false, r)
        } else {
            return Err(format!(
                "PUBLIC_URL '{raw}' should start with http:// or https://"
            ));
        };
        let (host, path) = match rest.find('/') {
            Some(i) => (&rest[..i], &rest[i..]),
            None => (rest, ""),
        };
        if !valid_host(host) {
            return Err(format!("PUBLIC_URL '{raw}' has an invalid host name"));
        }
        if !path.is_empty() && !valid_prefix(path) {
            return Err(format!(
                "PUBLIC_URL '{raw}' has an invalid path: use letters, digits and - _ . ~ in each segment"
            ));
        }
        Ok(PublicUrl {
            https,
            host: host.to_string(),
            path: path.to_string(),
        })
    }
}

/// A host (with an optional port) that is safe to put into a URL: letters,
/// digits, `.`, `-`, `_`, an IPv6 literal in brackets, and a numeric port.
pub fn valid_host(h: &str) -> bool {
    if h.is_empty() || h.len() > 255 {
        return false;
    }
    let (name, port) = match h.strip_prefix('[') {
        Some(rest) => match rest.split_once(']') {
            Some((v6, tail)) => (
                format!("[{v6}]"),
                match tail {
                    "" => None,
                    t => t.strip_prefix(':'),
                },
            ),
            None => return false,
        },
        None => match h.rsplit_once(':') {
            Some((n, p)) => (n.to_string(), Some(p)),
            None => (h.to_string(), None),
        },
    };
    let name_ok = if name.starts_with('[') {
        name[1..name.len() - 1]
            .chars()
            .all(|c| c.is_ascii_hexdigit() || c == ':' || c == '.')
    } else {
        !name.is_empty()
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_')
    };
    let port_ok = port.map_or(true, |p| {
        !p.is_empty() && p.len() <= 5 && p.chars().all(|c| c.is_ascii_digit())
    });
    name_ok && port_ok
}

/// A path prefix a proxy adds: `/a`, `/a/b`; URL-safe characters only, so it can
/// be placed in a URL or a JS string unescaped.
pub fn valid_prefix(p: &str) -> bool {
    p.starts_with('/')
        && !p.ends_with('/')
        && !p.contains("//")
        && p.split('/').skip(1).all(|seg| {
            !seg.is_empty()
                && seg != "."
                && seg != ".."
                && seg
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '~'))
        })
}

/// Loopback, private (RFC 1918), link-local and unique-local addresses: where a
/// reverse proxy of a home or Docker setup lives.
pub fn is_internal(ip: IpAddr) -> bool {
    let ip = match ip {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().map_or(IpAddr::V6(v6), IpAddr::V4),
        v4 => v4,
    };
    match ip {
        IpAddr::V4(v4) => v4.is_loopback() || v4.is_private() || v4.is_link_local(),
        IpAddr::V6(v6) => {
            let seg = v6.segments()[0];
            v6.is_loopback() || (seg & 0xfe00) == 0xfc00 || (seg & 0xffc0) == 0xfe80
        }
    }
}

/// The first element of a comma-separated header value.
fn first(v: &str) -> &str {
    v.split(',').next().unwrap_or("").trim()
}

fn header_str<'a>(h: &'a HeaderMap, name: &str) -> Option<&'a str> {
    h.get(name).and_then(|v| v.to_str().ok())
}

/// `proto=` and `host=` from the first element of a `Forwarded` header.
fn forwarded_parts(h: &HeaderMap) -> (Option<String>, Option<String>) {
    let Some(v) = h.get(header::FORWARDED).and_then(|v| v.to_str().ok()) else {
        return (None, None);
    };
    let (mut proto, mut host) = (None, None);
    for pair in first(v).split(';') {
        let Some((k, val)) = pair.split_once('=') else {
            continue;
        };
        let val = val.trim().trim_matches('"').to_string();
        match k.trim().to_ascii_lowercase().as_str() {
            "proto" => proto = Some(val.to_ascii_lowercase()),
            "host" => host = Some(val),
            _ => {}
        }
    }
    (proto, host)
}

/// What we worked out about one request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conn {
    /// The TCP peer.
    pub peer: IpAddr,
    /// The browser used HTTPS (so cookies can be `Secure`).
    pub https: bool,
    /// The host (and port) the browser used, if we could tell.
    pub host: Option<String>,
    /// A path the proxy adds in front of this app's own base (it strips it
    /// before forwarding): "" or e.g. "/memes".
    pub prefix: String,
    /// The forwarding headers were believed (the peer is a proxy of ours).
    pub forwarded_trusted: bool,
    /// The address came from `PUBLIC_URL`, not from headers.
    pub from_public_url: bool,
}

impl Conn {
    /// Work out the connection. `url_base` is the app's own `URL_BASE`;
    /// `direct_https` is whether this app itself serves HTTPS (`TLS_CERT`), the
    /// answer when no believed header says otherwise.
    pub fn from_parts(
        peer: IpAddr,
        headers: &HeaderMap,
        trusted: &[Net],
        public_url: Option<&PublicUrl>,
        url_base: &str,
        direct_https: bool,
    ) -> Conn {
        let own_host = header_str(headers, "host")
            .map(first)
            .filter(|h| valid_host(h))
            .map(str::to_string);

        if let Some(p) = public_url {
            // The path may include our own URL_BASE (the whole address people
            // type) -- only what comes before it is a proxy's prefix.
            let prefix = if url_base.is_empty() {
                p.path.clone()
            } else {
                p.path.strip_suffix(url_base).unwrap_or(&p.path).to_string()
            };
            return Conn {
                peer,
                https: p.https,
                host: Some(p.host.clone()),
                prefix,
                forwarded_trusted: false,
                from_public_url: true,
            };
        }

        let believed = trusted.iter().any(|n| n.contains(peer)) || is_internal(peer);
        if !believed {
            return Conn {
                peer,
                https: direct_https,
                host: own_host,
                prefix: String::new(),
                forwarded_trusted: false,
                from_public_url: false,
            };
        }

        let (f_proto, f_host) = forwarded_parts(headers);
        let proto = f_proto.or_else(|| {
            header_str(headers, "x-forwarded-proto").map(|v| first(v).to_ascii_lowercase())
        });
        let https = proto.as_deref().map_or(direct_https, |p| p == "https");

        let host = f_host
            .filter(|h| valid_host(h))
            .or_else(|| {
                let h = header_str(headers, "x-forwarded-host").map(first)?;
                if !valid_host(h) {
                    return None;
                }
                // A port sent separately, unless the host already has one or it is the default.
                let port = header_str(headers, "x-forwarded-port").map(first);
                Some(match port {
                    Some(p)
                        if !h.contains(':')
                            && !p.is_empty()
                            && p.chars().all(|c| c.is_ascii_digit())
                            && !matches!((https, p), (true, "443") | (false, "80")) =>
                    {
                        format!("{h}:{p}")
                    }
                    _ => h.to_string(),
                })
            })
            .or(own_host);

        let prefix = header_str(headers, "x-forwarded-prefix")
            .map(first)
            .map(|p| p.trim_end_matches('/'))
            .filter(|p| !p.is_empty() && valid_prefix(p))
            .map(str::to_string)
            .unwrap_or_default();

        Conn {
            peer,
            https,
            host,
            prefix,
            forwarded_trusted: true,
            from_public_url: false,
        }
    }

    /// `scheme://host`.
    pub fn origin(&self) -> Result<String, &'static str> {
        let host = self.host.as_deref().ok_or("Missing Host header")?;
        Ok(format!(
            "{}://{host}",
            if self.https { "https" } else { "http" }
        ))
    }

    /// Where the app lives, for URLs a browser will be sent back to: the
    /// origin, any proxy prefix, then our own `URL_BASE`.
    pub fn app_base(&self, url_base: &str) -> Result<String, &'static str> {
        Ok(format!("{}{}{url_base}", self.origin()?, self.prefix))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }
    fn nets(s: &str) -> Vec<Net> {
        crate::proxies::parse_list(s).unwrap()
    }
    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in pairs {
            h.insert(
                axum::http::HeaderName::from_bytes(k.as_bytes()).unwrap(),
                v.parse().unwrap(),
            );
        }
        h
    }
    fn conn(peer: &str, pairs: &[(&str, &str)]) -> Conn {
        Conn::from_parts(ip(peer), &headers(pairs), &[], None, "", false)
    }

    #[test]
    fn serving_tls_ourselves_makes_a_direct_connection_https() {
        let h = headers(&[("host", "memes.lan:8443")]);
        let c = Conn::from_parts(ip("203.0.113.9"), &h, &[], None, "", true);
        assert!(c.https);
        assert_eq!(c.origin().unwrap(), "https://memes.lan:8443");
        // a believed proxy that says the visitor used plain http wins
        let h = headers(&[("host", "x"), ("x-forwarded-proto", "http")]);
        assert!(!Conn::from_parts(ip("10.0.0.2"), &h, &[], None, "", true).https);
        // and one that says nothing leaves it https
        let h = headers(&[("host", "x")]);
        assert!(Conn::from_parts(ip("10.0.0.2"), &h, &[], None, "", true).https);
    }

    #[test]
    fn a_plain_request_is_http_on_its_own_host() {
        let c = conn("203.0.113.9", &[("host", "memes.lan:8080")]);
        assert!(!c.https && !c.forwarded_trusted);
        assert_eq!(c.origin().unwrap(), "http://memes.lan:8080");
    }

    #[test]
    fn a_proxy_on_the_lan_is_believed() {
        let c = conn(
            "172.18.0.5",
            &[
                ("host", "meme-arr:8080"),
                ("x-forwarded-proto", "https"),
                ("x-forwarded-host", "memes.example.com"),
            ],
        );
        assert!(c.https && c.forwarded_trusted);
        assert_eq!(c.origin().unwrap(), "https://memes.example.com");
    }

    #[test]
    fn a_client_on_the_internet_cannot_claim_https_or_another_host() {
        let c = conn(
            "203.0.113.9",
            &[
                ("host", "memes.example.com"),
                ("x-forwarded-proto", "https"),
                ("x-forwarded-host", "evil.example.net"),
                ("x-forwarded-prefix", "/evil"),
            ],
        );
        assert!(!c.https);
        assert_eq!(c.origin().unwrap(), "http://memes.example.com");
        assert_eq!(c.prefix, "");
    }

    #[test]
    fn a_listed_proxy_is_believed_even_on_a_public_address() {
        let h = headers(&[("host", "x"), ("x-forwarded-proto", "https")]);
        let c = Conn::from_parts(
            ip("203.0.113.9"),
            &h,
            &nets("203.0.113.0/24"),
            None,
            "",
            false,
        );
        assert!(c.https && c.forwarded_trusted);
    }

    #[test]
    fn the_standard_forwarded_header_works_and_wins() {
        let c = conn(
            "10.0.0.2",
            &[
                (
                    "forwarded",
                    r#"for=1.2.3.4;proto=https;host="memes.example.com:8443""#,
                ),
                ("x-forwarded-proto", "http"),
            ],
        );
        assert!(c.https);
        assert_eq!(c.origin().unwrap(), "https://memes.example.com:8443");
    }

    #[test]
    fn the_first_hop_of_a_chain_is_what_the_client_used() {
        let c = conn(
            "10.0.0.2",
            &[("host", "h"), ("x-forwarded-proto", "https, http")],
        );
        assert!(c.https);
    }

    #[test]
    fn a_separate_port_is_added_unless_it_is_the_default() {
        let c = conn(
            "10.0.0.2",
            &[
                ("x-forwarded-host", "m.example.com"),
                ("x-forwarded-proto", "https"),
                ("x-forwarded-port", "8443"),
            ],
        );
        assert_eq!(c.origin().unwrap(), "https://m.example.com:8443");
        let c = conn(
            "10.0.0.2",
            &[
                ("x-forwarded-host", "m.example.com"),
                ("x-forwarded-proto", "https"),
                ("x-forwarded-port", "443"),
            ],
        );
        assert_eq!(c.origin().unwrap(), "https://m.example.com");
        let c = conn(
            "10.0.0.2",
            &[
                ("x-forwarded-host", "m.example.com:9000"),
                ("x-forwarded-proto", "https"),
                ("x-forwarded-port", "8443"),
            ],
        );
        assert_eq!(c.origin().unwrap(), "https://m.example.com:9000");
    }

    #[test]
    fn a_bad_forwarded_host_is_ignored() {
        let c = conn(
            "10.0.0.2",
            &[
                ("host", "memes.lan"),
                ("x-forwarded-host", "evil.com/path?x=1"),
            ],
        );
        assert_eq!(c.origin().unwrap(), "http://memes.lan");
        let c = conn("10.0.0.2", &[("host", "bad host")]);
        assert!(c.origin().is_err());
    }

    #[test]
    fn a_stripped_prefix_comes_from_x_forwarded_prefix() {
        let c = conn(
            "10.0.0.2",
            &[("host", "h"), ("x-forwarded-prefix", "/memes/")],
        );
        assert_eq!(c.prefix, "/memes");
        assert_eq!(c.app_base("/meme-arr").unwrap(), "http://h/memes/meme-arr");
        // nonsense is dropped
        assert_eq!(
            conn(
                "10.0.0.2",
                &[("host", "h"), ("x-forwarded-prefix", "/a/../b")]
            )
            .prefix,
            ""
        );
        assert_eq!(
            conn(
                "10.0.0.2",
                &[("host", "h"), ("x-forwarded-prefix", "//evil.com")]
            )
            .prefix,
            ""
        );
    }

    #[test]
    fn public_url_overrides_every_header() {
        let p = PublicUrl::parse("https://memes.example.com/").unwrap();
        let h = headers(&[
            ("host", "internal:8080"),
            ("x-forwarded-proto", "http"),
            ("x-forwarded-host", "other"),
        ]);
        let c = Conn::from_parts(ip("203.0.113.9"), &h, &[], Some(&p), "", false);
        assert!(c.https && c.from_public_url);
        assert_eq!(c.app_base("").unwrap(), "https://memes.example.com");
    }

    #[test]
    fn public_url_with_a_path_may_include_the_url_base() {
        let p = PublicUrl::parse("https://example.com/apps/meme-arr").unwrap();
        // the proxy adds /apps and the app is also hosted under /meme-arr
        let c = Conn::from_parts(
            ip("10.0.0.2"),
            &HeaderMap::new(),
            &[],
            Some(&p),
            "/meme-arr",
            false,
        );
        assert_eq!(c.prefix, "/apps");
        assert_eq!(
            c.app_base("/meme-arr").unwrap(),
            "https://example.com/apps/meme-arr"
        );
        // the proxy strips everything, no URL_BASE
        let c = Conn::from_parts(ip("10.0.0.2"), &HeaderMap::new(), &[], Some(&p), "", false);
        assert_eq!(c.app_base("").unwrap(), "https://example.com/apps/meme-arr");
    }

    #[test]
    fn public_url_must_be_well_formed() {
        assert!(PublicUrl::parse("memes.example.com").is_err());
        assert!(PublicUrl::parse("ftp://x").is_err());
        assert!(PublicUrl::parse("https://bad host").is_err());
        assert!(PublicUrl::parse("https://x/a b").is_err());
        assert_eq!(
            PublicUrl::parse("http://192.168.1.5:8080").unwrap(),
            PublicUrl {
                https: false,
                host: "192.168.1.5:8080".into(),
                path: "".into()
            }
        );
    }

    #[test]
    fn hosts_and_prefixes_are_checked() {
        for ok in [
            "a.b",
            "a-b.c:8080",
            "[::1]:80",
            "[2001:db8::1]",
            "localhost",
        ] {
            assert!(valid_host(ok), "{ok}");
        }
        for bad in ["", "a b", "a/b", "a:b", "a?x", "[::1", ":80", "a:123456"] {
            assert!(!valid_host(bad), "{bad}");
        }
        assert!(valid_prefix("/a") && valid_prefix("/a/b-c_d.e~f"));
        for bad in ["", "/", "a", "/a/", "/a//b", "/a/..", "/a b", "/a?x"] {
            assert!(!valid_prefix(bad), "{bad}");
        }
    }

    #[test]
    fn internal_addresses() {
        for yes in [
            "127.0.0.1",
            "10.1.2.3",
            "172.16.0.1",
            "192.168.1.1",
            "169.254.1.1",
            "::1",
            "fd00::1",
            "fe80::1",
            "::ffff:10.0.0.1",
        ] {
            assert!(is_internal(ip(yes)), "{yes}");
        }
        for no in ["8.8.8.8", "203.0.113.9", "172.32.0.1", "2001:db8::1"] {
            assert!(!is_internal(ip(no)), "{no}");
        }
    }
}
