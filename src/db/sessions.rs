//! The admin password, login sessions, and the Plex/Authentik sign-in flows.

use super::*;

/// The admin password's stored Argon2id hash, if one has been set yet. A
/// missing hash means the app hasn't been through first-run setup —
/// `auth`-gated routes treat that as "setup required," not "locked out."
pub fn get_admin_password_hash(conn: &rusqlite::Connection) -> Result<Option<String>> {
    get_setting(conn, "admin_password_hash")
}

pub fn set_admin_password_hash(conn: &rusqlite::Connection, hash: &str) -> Result<()> {
    set_setting(conn, "admin_password_hash", hash)
}

/// This app's own stable Plex API client identifier — generated once and
/// reused forever after (Plex's own guidance: a random value, reused for
/// every request in a flow and across the app's lifetime, not regenerated
/// per attempt).
pub fn get_or_create_plex_client_identifier(conn: &rusqlite::Connection) -> Result<String> {
    if let Some(id) = get_setting(conn, "plex_client_identifier")? {
        return Ok(id);
    }
    let id = uuid::Uuid::new_v4().to_string();
    set_setting(conn, "plex_client_identifier", &id)?;
    Ok(id)
}

/// The single Plex account allowed to log in as this app's admin (set via
/// the Settings "Link your Plex account" action), if one has been linked.
pub fn get_plex_login_account(conn: &rusqlite::Connection) -> Result<Option<(String, String)>> {
    let uuid = get_setting(conn, "plex_login_account_uuid")?;
    let username = get_setting(conn, "plex_login_username")?;
    Ok(match (uuid, username) {
        (Some(uuid), Some(username)) if !uuid.is_empty() => Some((uuid, username)),
        _ => None,
    })
}

pub fn set_plex_login_account(
    conn: &rusqlite::Connection,
    account_uuid: &str,
    username: &str,
) -> Result<()> {
    set_setting(conn, "plex_login_account_uuid", account_uuid)?;
    set_setting(conn, "plex_login_username", username)?;
    Ok(())
}

pub fn delete_plex_login_account(conn: &rusqlite::Connection) -> Result<()> {
    delete_setting(conn, "plex_login_account_uuid")?;
    delete_setting(conn, "plex_login_username")?;
    Ok(())
}

pub fn create_plex_flow(
    conn: &rusqlite::Connection,
    pin_id: i64,
    code: &str,
    purpose: &str,
    created_at: i64,
    expires_at: i64,
) -> Result<()> {
    conn.execute(
        "INSERT INTO plex_flows(pin_id, code, purpose, created_at, expires_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![pin_id, code, purpose, created_at, expires_at],
    )?;
    Ok(())
}

/// Look up and consume (delete) an in-progress flow by PIN id — one-time
/// use, same reasoning as `take_oidc_flow`. By the time Plex redirects the
/// browser back to our callback, the user has already approved (or denied)
/// the login at Plex, so a single check here is enough; no polling needed.
pub fn take_plex_flow(
    conn: &rusqlite::Connection,
    pin_id: i64,
    now: i64,
) -> Result<Option<(String, String)>> {
    let found = conn
        .query_row(
            "SELECT code, purpose FROM plex_flows WHERE pin_id = ?1 AND expires_at > ?2",
            params![pin_id, now],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
        )
        .optional()?;
    conn.execute("DELETE FROM plex_flows WHERE pin_id = ?1", params![pin_id])?;
    Ok(found)
}

pub fn prune_expired_plex_flows(conn: &rusqlite::Connection, now: i64) -> Result<()> {
    conn.execute(
        "DELETE FROM plex_flows WHERE expires_at <= ?1",
        params![now],
    )?;
    Ok(())
}

pub fn create_session(
    conn: &rusqlite::Connection,
    token_hash: &str,
    created_at: i64,
    expires_at: i64,
) -> Result<()> {
    conn.execute(
        "INSERT INTO sessions(token_hash, created_at, expires_at) VALUES (?1, ?2, ?3)",
        params![token_hash, created_at, expires_at],
    )?;
    Ok(())
}

/// Whether `token_hash` names a session that exists and hasn't expired.
/// Expired-but-present sessions are left for `prune_expired_sessions` to
/// sweep rather than deleted here, so a read never also needs a write.
pub fn session_valid(conn: &rusqlite::Connection, token_hash: &str, now: i64) -> Result<bool> {
    Ok(conn
        .query_row(
            "SELECT 1 FROM sessions WHERE token_hash = ?1 AND expires_at > ?2",
            params![token_hash, now],
            |_| Ok(()),
        )
        .optional()?
        .is_some())
}

/// End one session (logout).
pub fn delete_session(conn: &rusqlite::Connection, token_hash: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM sessions WHERE token_hash = ?1",
        params![token_hash],
    )?;
    Ok(())
}

/// Sweep expired sessions — called opportunistically on login rather than on
/// a timer, since this app has at most one real user and logins are rare
/// enough that the table never grows large between sweeps.
pub fn prune_expired_sessions(conn: &rusqlite::Connection, now: i64) -> Result<()> {
    conn.execute("DELETE FROM sessions WHERE expires_at <= ?1", params![now])?;
    Ok(())
}

/// Start tracking one OIDC login attempt (state -> nonce + PKCE verifier).
pub fn create_oidc_flow(
    conn: &rusqlite::Connection,
    state: &str,
    nonce: &str,
    pkce_verifier: &str,
    created_at: i64,
    expires_at: i64,
) -> Result<()> {
    conn.execute(
        "INSERT INTO oidc_flows(state, nonce, pkce_verifier, created_at, expires_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![state, nonce, pkce_verifier, created_at, expires_at],
    )?;
    Ok(())
}

/// Look up and consume (delete) an in-progress OIDC flow by its state token
/// — one-time use, same as the `code` it's paired with. Returns `None` if
/// the state is unknown or expired (the callback should treat both the
/// same: reject the login).
pub fn take_oidc_flow(
    conn: &rusqlite::Connection,
    state: &str,
    now: i64,
) -> Result<Option<(String, String)>> {
    let found = conn
        .query_row(
            "SELECT nonce, pkce_verifier FROM oidc_flows WHERE state = ?1 AND expires_at > ?2",
            params![state, now],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
        )
        .optional()?;
    conn.execute("DELETE FROM oidc_flows WHERE state = ?1", params![state])?;
    Ok(found)
}

/// Sweep expired/abandoned OIDC flows — opportunistic, same reasoning as
/// `prune_expired_sessions`.
pub fn prune_expired_oidc_flows(conn: &rusqlite::Connection, now: i64) -> Result<()> {
    conn.execute(
        "DELETE FROM oidc_flows WHERE expires_at <= ?1",
        params![now],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admin_password_hash_roundtrip() {
        let pool = mem_pool();
        let conn = pool.get().unwrap();
        assert_eq!(get_admin_password_hash(&conn).unwrap(), None);
        set_admin_password_hash(&conn, "argon2-hash-goes-here").unwrap();
        assert_eq!(
            get_admin_password_hash(&conn).unwrap().as_deref(),
            Some("argon2-hash-goes-here")
        );
        // Setting it again (a password change) overwrites, doesn't error.
        set_admin_password_hash(&conn, "a-new-hash").unwrap();
        assert_eq!(
            get_admin_password_hash(&conn).unwrap().as_deref(),
            Some("a-new-hash")
        );
    }

    #[test]
    fn plex_client_identifier_is_generated_once_and_persists() {
        let pool = mem_pool();
        let conn = pool.get().unwrap();
        let a = get_or_create_plex_client_identifier(&conn).unwrap();
        let b = get_or_create_plex_client_identifier(&conn).unwrap();
        assert_eq!(a, b);
        assert!(!a.is_empty());
    }

    #[test]
    fn plex_login_account_roundtrip() {
        let pool = mem_pool();
        let conn = pool.get().unwrap();
        assert!(get_plex_login_account(&conn).unwrap().is_none());

        set_plex_login_account(&conn, "abc-uuid", "plexuser").unwrap();
        assert_eq!(
            get_plex_login_account(&conn).unwrap(),
            Some(("abc-uuid".to_string(), "plexuser".to_string()))
        );

        delete_plex_login_account(&conn).unwrap();
        assert!(get_plex_login_account(&conn).unwrap().is_none());
    }

    #[test]
    fn plex_flow_is_one_time_use() {
        let pool = mem_pool();
        let conn = pool.get().unwrap();
        create_plex_flow(&conn, 42, "code1", "login", 0, 1000).unwrap();

        let found = take_plex_flow(&conn, 42, 500).unwrap();
        assert_eq!(found, Some(("code1".to_string(), "login".to_string())));
        assert_eq!(take_plex_flow(&conn, 42, 500).unwrap(), None);
    }

    #[test]
    fn plex_flow_expired_is_treated_as_missing() {
        let pool = mem_pool();
        let conn = pool.get().unwrap();
        create_plex_flow(&conn, 42, "code1", "link", 0, 100).unwrap();
        assert_eq!(take_plex_flow(&conn, 42, 500).unwrap(), None);
    }

    #[test]
    fn prune_expired_plex_flows_removes_only_expired() {
        let pool = mem_pool();
        let conn = pool.get().unwrap();
        create_plex_flow(&conn, 1, "c", "login", 0, 100).unwrap();
        create_plex_flow(&conn, 2, "c", "login", 0, 9999).unwrap();

        prune_expired_plex_flows(&conn, 500).unwrap();

        assert!(take_plex_flow(&conn, 1, 50).unwrap().is_none());
        assert!(take_plex_flow(&conn, 2, 600).unwrap().is_some());
    }

    #[test]
    fn session_lifecycle() {
        let pool = mem_pool();
        let conn = pool.get().unwrap();
        create_session(&conn, "tok1", 1000, 2000).unwrap();

        assert!(session_valid(&conn, "tok1", 1500).unwrap());
        assert!(!session_valid(&conn, "tok1", 2000).unwrap()); // expires_at is exclusive
        assert!(!session_valid(&conn, "nonexistent", 1500).unwrap());

        delete_session(&conn, "tok1").unwrap();
        assert!(!session_valid(&conn, "tok1", 1500).unwrap());
    }

    #[test]
    fn prune_expired_sessions_removes_only_expired() {
        let pool = mem_pool();
        let conn = pool.get().unwrap();
        create_session(&conn, "expired", 0, 100).unwrap();
        create_session(&conn, "still-good", 0, 9999).unwrap();

        prune_expired_sessions(&conn, 500).unwrap();

        assert!(!session_valid(&conn, "expired", 50).unwrap()); // gone entirely now
        assert!(session_valid(&conn, "still-good", 600).unwrap());
    }

    #[test]
    fn oidc_flow_is_one_time_use() {
        let pool = mem_pool();
        let conn = pool.get().unwrap();
        create_oidc_flow(&conn, "state1", "nonce1", "verifier1", 0, 1000).unwrap();

        let found = take_oidc_flow(&conn, "state1", 500).unwrap();
        assert_eq!(found, Some(("nonce1".to_string(), "verifier1".to_string())));

        // Second lookup of the same state finds nothing -- consumed above.
        assert_eq!(take_oidc_flow(&conn, "state1", 500).unwrap(), None);
    }

    #[test]
    fn oidc_flow_expired_is_treated_as_missing() {
        let pool = mem_pool();
        let conn = pool.get().unwrap();
        create_oidc_flow(&conn, "state1", "nonce1", "verifier1", 0, 100).unwrap();
        assert_eq!(take_oidc_flow(&conn, "state1", 500).unwrap(), None);
    }

    #[test]
    fn prune_expired_oidc_flows_removes_only_expired() {
        let pool = mem_pool();
        let conn = pool.get().unwrap();
        create_oidc_flow(&conn, "expired", "n", "v", 0, 100).unwrap();
        create_oidc_flow(&conn, "still-good", "n", "v", 0, 9999).unwrap();

        prune_expired_oidc_flows(&conn, 500).unwrap();

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM oidc_flows", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);
        assert!(take_oidc_flow(&conn, "still-good", 600).unwrap().is_some());
    }
}
