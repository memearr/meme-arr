//! HTTPS to the servers this app talks to (Plex, Jellyfin, Bazarr, Authentik,
//! plex.tv, GitHub): which certificates to trust.
//!
//! Servers on a home network often use a certificate from a private CA or a
//! self-signed one, which a normal client refuses. Two settings cover it:
//!
//! * `EXTRA_CA_CERTS`: PEM files (or folders of them) whose certificates are
//!   trusted in addition to the system's. The right fix: trust *your* CA and
//!   keep checking everything.
//! * `TLS_INSECURE_HOSTS`: host names whose certificate is not checked at all.
//!   A last resort, for one named server at a time; it applies only to the
//!   servers you configured, never to plex.tv or GitHub.

use anyhow::{bail, Context, Result};
use std::path::Path;
use std::sync::OnceLock;

/// What to trust, set once at startup.
#[derive(Debug, Clone, Default)]
pub struct Options {
    /// Each entry is the bytes of one PEM file (one or more certificates).
    pub extra_ca_pem: Vec<Vec<u8>>,
    /// Lower-case host names (`plex.lan`, `192.168.1.5`, `*.lan`, or `*` for all).
    pub insecure_hosts: Vec<String>,
}

static OPTIONS: OnceLock<Options> = OnceLock::new();

/// Set the options for the life of the process (the first call wins).
pub fn init(opts: Options) {
    let _ = OPTIONS.set(opts);
}

fn options() -> Options {
    OPTIONS.get().cloned().unwrap_or_default()
}

/// Reload the certificate this app serves when its files change (a renewal by
/// Let's Encrypt, certbot, a reverse proxy's ACME job...), without a restart.
/// Checks the files' modification times once a minute.
pub async fn watch_certificate(
    config: axum_server::tls_rustls::RustlsConfig,
    cert: std::path::PathBuf,
    key: std::path::PathBuf,
) {
    let stamp = |p: &std::path::Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    let mut seen = (stamp(&cert), stamp(&key));
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        let now = (stamp(&cert), stamp(&key));
        if now == seen {
            continue;
        }
        match config.reload_from_pem_file(&cert, &key).await {
            Ok(()) => {
                tracing::info!("reloaded the HTTPS certificate from {}", cert.display());
                seen = now;
            }
            // A half-written renewal: keep serving the old one and try again.
            Err(e) => tracing::warn!("couldn't reload the HTTPS certificate yet: {e:#}"),
        }
    }
}

/// Read `EXTRA_CA_CERTS`: comma-separated paths to PEM files or to folders
/// holding `.pem`, `.crt` or `.cer` files. A path that can't be read, or a
/// file with no certificate in it, is an error, so a typo isn't silent.
pub fn load_extra_ca(spec: &str) -> Result<Vec<Vec<u8>>> {
    let mut out = Vec::new();
    for raw in spec.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        let path = Path::new(raw);
        let files = if path.is_dir() {
            let mut v: Vec<_> = std::fs::read_dir(path)
                .with_context(|| format!("reading the certificate folder {raw}"))?
                .flatten()
                .map(|e| e.path())
                .filter(|p| {
                    p.extension()
                        .and_then(|e| e.to_str())
                        .is_some_and(|e| matches!(e.to_lowercase().as_str(), "pem" | "crt" | "cer"))
                })
                .collect();
            v.sort();
            if v.is_empty() {
                bail!("EXTRA_CA_CERTS folder {raw} has no .pem, .crt or .cer files");
            }
            v
        } else {
            vec![path.to_path_buf()]
        };
        for f in files {
            let bytes = std::fs::read(&f)
                .with_context(|| format!("reading the CA certificate {}", f.display()))?;
            let certs = reqwest::Certificate::from_pem_bundle(&bytes)
                .with_context(|| format!("{} isn't a PEM certificate", f.display()))?;
            if certs.is_empty() {
                bail!("{} holds no certificate", f.display());
            }
            out.push(bytes);
        }
    }
    Ok(out)
}

/// Parse `TLS_INSECURE_HOSTS`: comma- or space-separated host names.
pub fn parse_hosts(list: &str) -> Vec<String> {
    list.split(|c: char| c == ',' || c.is_whitespace())
        .map(|h| h.trim().to_lowercase())
        .filter(|h| !h.is_empty())
        .collect()
}

/// The host of a URL, lower-cased, without port or brackets.
fn host_of(url: &str) -> Option<String> {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let authority = rest.split(['/', '?', '#']).next()?;
    let authority = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    let host = if let Some(v6) = authority.strip_prefix('[') {
        v6.split(']').next()?
    } else {
        authority.split(':').next()?
    };
    (!host.is_empty()).then(|| host.to_lowercase())
}

/// Is `url`'s host one whose certificate is not checked?
pub fn is_insecure(hosts: &[String], url: &str) -> bool {
    let Some(host) = host_of(url) else {
        return false;
    };
    hosts.iter().any(|h| {
        h == "*"
            || *h == host
            || h.strip_prefix("*.")
                .is_some_and(|suffix| host.ends_with(&format!(".{suffix}")))
    })
}

/// An HTTP client for a server the user configured at `url` (`Some`), or for a
/// public service (`None`: plex.tv, GitHub): the extra CAs are trusted for
/// both; certificate checking is switched off only for a listed host.
pub fn client(url: Option<&str>) -> reqwest::ClientBuilder {
    client_with(&options(), url)
}

/// `client` with the options given (rather than the process-wide ones).
pub fn client_with(opts: &Options, url: Option<&str>) -> reqwest::ClientBuilder {
    let mut b = reqwest::Client::builder();
    for pem in &opts.extra_ca_pem {
        for cert in reqwest::Certificate::from_pem_bundle(pem).unwrap_or_default() {
            b = b.add_root_certificate(cert);
        }
    }
    if url.is_some_and(|u| is_insecure(&opts.insecure_hosts, u)) {
        b = b.danger_accept_invalid_certs(true);
    }
    b
}

/// The same for the OpenID Connect library, which carries its own copy of
/// `reqwest`. `url` is the issuer.
pub fn oidc_client(url: &str) -> openidconnect::reqwest::ClientBuilder {
    let opts = options();
    let mut b = openidconnect::reqwest::ClientBuilder::new();
    for pem in &opts.extra_ca_pem {
        for cert in openidconnect::reqwest::Certificate::from_pem_bundle(pem).unwrap_or_default() {
            b = b.add_root_certificate(cert);
        }
    }
    if is_insecure(&opts.insecure_hosts, url) {
        b = b.danger_accept_invalid_certs(true);
    }
    b
}

/// A sentence to add to a failed HTTPS request's message when the failure
/// looks like a certificate problem, saying what to do about it.
pub fn hint(error_text: &str) -> Option<&'static str> {
    let t = error_text.to_lowercase();
    let cert = [
        "certificate",
        "unknownissuer",
        "unknown issuer",
        "self-signed",
        "self signed",
        "notvalidforname",
        "invalid peer",
    ];
    cert.iter().any(|k| t.contains(k)).then_some(
        "The server's HTTPS certificate isn't trusted. Add the certificate authority that signed it \
         with EXTRA_CA_CERTS (a PEM file or folder), or, as a last resort for that one server, list its \
         host name in TLS_INSECURE_HOSTS.",
    )
}

/// `message` with the certificate hint appended when it applies.
pub fn explain(message: String) -> String {
    match hint(&message) {
        Some(h) => format!("{message}. {h}"),
        None => message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A throwaway self-signed certificate, as PEM.
    fn test_cert() -> String {
        rcgen::generate_simple_self_signed(vec!["x.test".to_string()])
            .unwrap()
            .cert
            .pem()
    }

    #[test]
    fn hosts_are_taken_from_urls() {
        assert_eq!(
            host_of("https://Plex.LAN:32400/x?y").as_deref(),
            Some("plex.lan")
        );
        assert_eq!(
            host_of("http://user:pw@host.example/").as_deref(),
            Some("host.example")
        );
        assert_eq!(host_of("https://[::1]:8920").as_deref(), Some("::1"));
        assert_eq!(host_of("192.168.1.5:8096").as_deref(), Some("192.168.1.5"));
        assert_eq!(host_of("https://"), None);
    }

    #[test]
    fn only_listed_hosts_skip_certificate_checks() {
        let hosts = parse_hosts("Plex.lan, 192.168.1.5  *.home.arpa");
        assert!(is_insecure(&hosts, "https://plex.lan:32400"));
        assert!(is_insecure(&hosts, "https://192.168.1.5:8920/x"));
        assert!(is_insecure(&hosts, "https://jelly.home.arpa"));
        assert!(
            !is_insecure(&hosts, "https://home.arpa"),
            "wildcard needs a subdomain"
        );
        assert!(!is_insecure(&hosts, "https://plex.tv"));
        assert!(!is_insecure(&hosts, "https://notplex.lan"));
        assert!(!is_insecure(&[], "https://plex.lan"));
        assert!(is_insecure(&parse_hosts("*"), "https://anything.example"));
    }

    #[test]
    fn extra_cas_load_from_files_and_folders() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("my-ca.pem");
        std::fs::write(&file, test_cert()).unwrap();
        std::fs::write(dir.path().join("notes.txt"), "not a certificate").unwrap();

        let one = load_extra_ca(&file.to_string_lossy()).unwrap();
        assert_eq!(one.len(), 1);
        // a folder: only .pem/.crt/.cer files are read
        let many = load_extra_ca(&dir.path().to_string_lossy()).unwrap();
        assert_eq!(many.len(), 1);
        // two entries
        let both = load_extra_ca(&format!("{}, {}", file.display(), dir.path().display())).unwrap();
        assert_eq!(both.len(), 2);
        assert!(load_extra_ca("").unwrap().is_empty());
    }

    #[test]
    fn a_bad_ca_setting_is_an_error_not_a_silent_skip() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load_extra_ca(&dir.path().join("missing.pem").to_string_lossy()).is_err());
        let junk = dir.path().join("junk.pem");
        std::fs::write(&junk, "hello").unwrap();
        assert!(load_extra_ca(&junk.to_string_lossy()).is_err());
        let empty = tempfile::tempdir().unwrap();
        assert!(load_extra_ca(&empty.path().to_string_lossy()).is_err());
    }

    #[test]
    fn a_certificate_failure_gets_a_hint_and_others_do_not() {
        assert!(hint("error sending request: invalid peer certificate: UnknownIssuer").is_some());
        assert!(hint("the certificate is self-signed").is_some());
        assert!(hint("connection refused").is_none());
        assert!(explain("tcp connect error".into()) == "tcp connect error");
        assert!(explain("invalid peer certificate".into()).contains("EXTRA_CA_CERTS"));
    }

    // ---- against a real HTTPS server with a certificate of its own CA-less making ----

    /// Serve "ok" over HTTPS on a free port with a fresh self-signed
    /// certificate for `localhost`; returns the URL and the certificate (PEM).
    async fn https_server() -> (String, String) {
        let ck = rcgen::generate_simple_self_signed(vec!["localhost".to_string()]).unwrap();
        let cert_pem = ck.cert.pem();
        let key_pem = ck.signing_key.serialize_pem();
        let dir = tempfile::tempdir().unwrap();
        let (c, k) = (dir.path().join("c.pem"), dir.path().join("k.pem"));
        std::fs::write(&c, &cert_pem).unwrap();
        std::fs::write(&k, &key_pem).unwrap();
        let cfg = axum_server::tls_rustls::RustlsConfig::from_pem_file(&c, &k)
            .await
            .unwrap();
        let app = axum::Router::new().route("/", axum::routing::get(|| async { "ok" }));
        let handle = axum_server::Handle::<std::net::SocketAddr>::new();
        let h2 = handle.clone();
        tokio::spawn(async move {
            let _keep = dir; // the files stay until the server is gone
            axum_server::bind_rustls("127.0.0.1:0".parse().unwrap(), cfg)
                .handle(h2)
                .serve(app.into_make_service())
                .await
                .unwrap();
        });
        let addr = handle.listening().await.expect("server is listening");
        (format!("https://localhost:{}", addr.port()), cert_pem)
    }

    #[tokio::test]
    async fn an_unknown_certificate_is_refused_then_trusted_by_ca_or_by_host() {
        let (url, cert_pem) = https_server().await;
        let get = |opts: Options| {
            let url = url.clone();
            async move {
                client_with(&opts, Some(&url))
                    .build()
                    .unwrap()
                    .get(&url)
                    .send()
                    .await
                    .map(|r| r.status().is_success())
                    .map_err(|e| format!("{e:?}"))
            }
        };

        // 1. nothing trusted: refused, and the message earns our hint
        let err = get(Options::default()).await.unwrap_err();
        assert!(
            hint(&err).is_some(),
            "a certificate error should be recognised: {err}"
        );

        // 2. its own certificate trusted as a CA: works, with checking still on
        let trusted = Options {
            extra_ca_pem: vec![cert_pem.clone().into_bytes()],
            ..Default::default()
        };
        assert_eq!(get(trusted).await, Ok(true));

        // 3. the host listed as insecure: works without it
        let lax = Options {
            insecure_hosts: parse_hosts("localhost"),
            ..Default::default()
        };
        assert_eq!(get(lax).await, Ok(true));

        // 4. another host listed: still refused
        let other = Options {
            insecure_hosts: parse_hosts("elsewhere.lan"),
            ..Default::default()
        };
        assert!(get(other).await.is_err());
    }

    #[tokio::test]
    async fn a_public_service_is_never_exempted() {
        let (url, _cert) = https_server().await;
        let opts = Options {
            insecure_hosts: parse_hosts("*"),
            ..Default::default()
        };
        // `None` = a public service such as plex.tv: the exemption does not apply
        let err = client_with(&opts, None)
            .build()
            .unwrap()
            .get(&url)
            .send()
            .await
            .unwrap_err();
        assert!(hint(&format!("{err:?}")).is_some());
    }

    #[test]
    fn clients_build_with_and_without_options() {
        assert!(client(None).build().is_ok());
        assert!(client(Some("https://plex.lan")).build().is_ok());
        assert!(oidc_client("https://auth.example.com/").build().is_ok());
    }
}
