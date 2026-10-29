//! Single-admin authentication: password hashing, session tokens.
//!
//! Deliberately mirrors Sonarr's approach (checked before building this,
//! not assumed): the credential lives in the app's own database, never a
//! config file, so it can't be read back — only replaced. Recovery is
//! config-driven (see `config::Config::admin_password_reset` /
//! `RESET_ADMIN_PASSWORD`), the same shape as editing Sonarr's
//! `config.xml` and restarting.
//!
//! Password hashing is Argon2id (slow, intentionally — resists brute force
//! on a stolen hash). Session tokens are a separate, fast-hashed (SHA-256)
//! random value: using the slow password hasher for every authenticated
//! request would make the whole app sluggish, so this is the standard
//! split (bcrypt/argon2 for the one password check at login, a fast hash
//! for the many session lookups afterward).

use anyhow::{anyhow, Result};
use argon2::password_hash::phc::PasswordHash;
use argon2::password_hash::{PasswordHasher, PasswordVerifier};
use argon2::Argon2;
use sha2::{Digest, Sha256};

/// Hash a password for storage (a self-contained PHC string — salt and
/// parameters travel with it, so verifying never needs them passed back in).
/// The salt is generated from the OS RNG inside `hash_password`.
pub fn hash_password(password: &str) -> Result<String> {
    Argon2::default()
        .hash_password(password.as_bytes())
        .map(|h| h.to_string())
        .map_err(|e| anyhow!("hashing password: {e}"))
}

/// Check a password against a stored hash from [`hash_password`].
pub fn verify_password(password: &str, stored_hash: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(stored_hash) else {
        return false;
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
}

/// A new random session token (the raw value that goes in the cookie — only
/// its [`hash_token`] is ever stored server-side). 128 bits of randomness
/// from the OS CSPRNG via `uuid`'s v4 generator, concatenated twice for a
/// comfortable margin over a typical single-UUID session ID.
pub fn new_session_token() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}

/// Fast, deterministic hash of a session token for server-side storage —
/// never the slow password hasher here (see module docs).
pub fn hash_token(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    // Lowercase hex, written out: sha2 0.11's output array no longer
    // implements `LowerHex`. The format must stay byte-for-byte what it was,
    // because these are the values already stored in the sessions table.
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Prefix on generated API keys, so a leaked one is recognizable on sight.
pub const API_KEY_PREFIX: &str = "memearr_";

pub fn new_api_key() -> String {
    format!("{API_KEY_PREFIX}{}", new_session_token())
}

/// The API key is for dashboards and monitoring (Homepage widgets, scripts),
/// not full control: it can only `GET` this short list. Everything else --
/// settings (which hold the Plex token), backups (secrets), exports, anything
/// that changes state -- needs a real login. An allowlist, so a new endpoint
/// is off-limits to keys until someone deliberately adds it here.
pub fn api_key_may_access(method: &axum::http::Method, path: &str) -> bool {
    *method == axum::http::Method::GET
        && matches!(
            path,
            "/api/tasks"
                | "/api/health"
                | "/api/index/status"
                | "/api/libraries"
                | "/api/libraries/stats"
                | "/api/system/storage"
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A hash written by the PREVIOUS build (argon2 0.5) for the password
    /// below, taken from a real database. An upgrade of the hashing library
    /// must keep accepting hashes people already have stored.
    const LEGACY_HASH: &str = "$argon2id$v=19$m=19456,t=2,p=1$XY/U/JBYKTNyUO9LEql0Ew$8mGuVJxLHrBCiVyepyEifKsOaEOYk9kmrkYWQyeTwrk";

    #[test]
    fn a_hash_written_by_the_previous_library_version_still_verifies() {
        assert!(verify_password("legacy-pw-123", LEGACY_HASH));
        assert!(!verify_password("legacy-pw-124", LEGACY_HASH));
        assert!(!verify_password("", LEGACY_HASH));
    }

    #[test]
    fn new_hashes_have_the_same_format_and_round_trip() {
        let h = hash_password("a new password").unwrap();
        assert!(h.starts_with("$argon2id$v=19$m=19456,t=2,p=1$"), "{h}");
        assert!(verify_password("a new password", &h));
        assert!(!verify_password("another", &h));
        // A fresh random salt every time.
        assert_ne!(h, hash_password("a new password").unwrap());
        // Garbage in the database is "not a match", never a panic.
        assert!(!verify_password("x", "not a phc string"));
        assert!(!verify_password("x", ""));
    }

    #[test]
    fn session_token_hashes_are_unchanged_lowercase_sha256_hex() {
        // Standard SHA-256 test vectors. These hashes are what the sessions
        // table and the API-key check already hold, so they must not change.
        assert_eq!(
            hash_token("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            hash_token(""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        let t = hash_token(&new_session_token());
        assert_eq!(t.len(), 64);
        assert!(t
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)));
    }

    #[test]
    fn api_keys_are_prefixed_and_unique() {
        let (a, b) = (new_api_key(), new_api_key());
        assert!(a.starts_with(API_KEY_PREFIX) && a.len() > 60);
        assert_ne!(a, b);
    }

    #[test]
    fn api_keys_are_read_only_and_limited_to_the_allowlist() {
        use axum::http::Method;
        assert!(api_key_may_access(&Method::GET, "/api/health"));
        assert!(api_key_may_access(&Method::GET, "/api/tasks"));
        // Reads of secrets, and any write, are off-limits.
        for path in [
            "/api/settings",
            "/api/backups",
            "/api/backups/settings-20261025-035235.json",
            "/api/export",
            "/api/activity/log",
        ] {
            assert!(!api_key_may_access(&Method::GET, path), "{path}");
        }
        for m in [Method::POST, Method::PUT, Method::DELETE] {
            assert!(
                !api_key_may_access(&m, "/api/tasks/library_scan/run"),
                "{m}"
            );
            assert!(!api_key_may_access(&m, "/api/health"), "{m}");
        }
        // Not a prefix match.
        assert!(!api_key_may_access(&Method::GET, "/api/health/check"));
        assert!(!api_key_may_access(&Method::GET, "/api/tasks/library_scan"));
    }

    #[test]
    fn password_hash_roundtrips() {
        let hash = hash_password("correct horse battery staple").unwrap();
        assert!(verify_password("correct horse battery staple", &hash));
        assert!(!verify_password("wrong password", &hash));
    }

    #[test]
    fn password_hash_is_salted() {
        // Same password, hashed twice, should produce different stored
        // hashes (random salt) -- both must still verify correctly.
        let a = hash_password("same password").unwrap();
        let b = hash_password("same password").unwrap();
        assert_ne!(a, b);
        assert!(verify_password("same password", &a));
        assert!(verify_password("same password", &b));
    }

    #[test]
    fn verify_password_rejects_garbage_hash() {
        assert!(!verify_password("anything", "not a real phc hash"));
    }

    #[test]
    fn session_tokens_are_unique_and_hash_deterministically() {
        let a = new_session_token();
        let b = new_session_token();
        assert_ne!(a, b);
        assert_eq!(hash_token(&a), hash_token(&a));
        assert_ne!(hash_token(&a), hash_token(&b));
    }
}
