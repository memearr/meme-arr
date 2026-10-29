//! Key-value settings, path mappings, and the Authentik settings.

use super::*;

/// A server-path -> container-path mapping row. (The JSON/DB field is still
/// called `plex_prefix`, from before Jellyfin support: renaming it would break
/// the API, saved settings and backups, so only the Rust-side wording is neutral.)
#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct Mapping {
    #[serde(default)]
    pub plex_prefix: String,
    #[serde(default)]
    pub container_path: String,
    #[serde(default)]
    pub label: Option<String>,
}

pub fn get_setting(conn: &rusqlite::Connection, key: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT value FROM settings WHERE key = ?1",
            params![key],
            |r| r.get::<_, String>(0),
        )
        .ok())
}

pub fn set_setting(conn: &rusqlite::Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO settings(key, value) VALUES(?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

pub fn delete_setting(conn: &rusqlite::Connection, key: &str) -> Result<()> {
    conn.execute("DELETE FROM settings WHERE key = ?1", params![key])?;
    Ok(())
}

/// Authentik OIDC settings, if all three pieces have been configured (kept
/// DB-only rather than cached on `AppState` — login is infrequent, not a
/// hot path that needs an in-memory snapshot the way Plex settings do).
pub fn get_authentik_settings(
    conn: &rusqlite::Connection,
) -> Result<Option<crate::oidc::OidcSettings>> {
    let issuer_url = get_setting(conn, "authentik_issuer_url")?;
    let client_id = get_setting(conn, "authentik_client_id")?;
    let client_secret = get_setting(conn, "authentik_client_secret")?;
    Ok(match (issuer_url, client_id, client_secret) {
        (Some(issuer_url), Some(client_id), Some(client_secret))
            if !issuer_url.is_empty() && !client_id.is_empty() && !client_secret.is_empty() =>
        {
            Some(crate::oidc::OidcSettings {
                issuer_url,
                client_id,
                client_secret,
            })
        }
        _ => None,
    })
}

pub fn set_authentik_settings(
    conn: &rusqlite::Connection,
    issuer_url: &str,
    client_id: &str,
    client_secret: &str,
) -> Result<()> {
    set_setting(conn, "authentik_issuer_url", issuer_url)?;
    set_setting(conn, "authentik_client_id", client_id)?;
    set_setting(conn, "authentik_client_secret", client_secret)?;
    Ok(())
}

pub fn delete_authentik_settings(conn: &rusqlite::Connection) -> Result<()> {
    delete_setting(conn, "authentik_issuer_url")?;
    delete_setting(conn, "authentik_client_id")?;
    delete_setting(conn, "authentik_client_secret")?;
    Ok(())
}

pub fn list_mappings(conn: &rusqlite::Connection) -> Result<Vec<Mapping>> {
    let mut stmt = conn.prepare(
        "SELECT plex_prefix, container_path, label FROM path_mapping ORDER BY position, id",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(Mapping {
            plex_prefix: r.get(0)?,
            container_path: r.get(1)?,
            label: r.get(2)?,
        })
    })?;
    Ok(rows.filter_map(Result::ok).collect())
}

/// Replace all mappings with the supplied ordered list (empty prefixes skipped).
pub fn replace_mappings(conn: &mut rusqlite::Connection, mappings: &[Mapping]) -> Result<()> {
    let tx = conn.transaction()?;
    tx.execute("DELETE FROM path_mapping", [])?;
    {
        let mut stmt = tx.prepare(
            "INSERT INTO path_mapping(position, plex_prefix, container_path, label)
             VALUES(?1, ?2, ?3, ?4)",
        )?;
        for (i, m) in mappings.iter().enumerate() {
            if m.plex_prefix.trim().is_empty() || m.container_path.trim().is_empty() {
                continue;
            }
            stmt.execute(params![
                i as i64,
                m.plex_prefix.trim(),
                m.container_path.trim(),
                m.label
            ])?;
        }
    }
    tx.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_crud_roundtrip() {
        let pool = mem_pool();
        let conn = pool.get().unwrap();
        assert_eq!(get_setting(&conn, "plex_url").unwrap(), None);
        set_setting(&conn, "plex_url", "http://plex:32400").unwrap();
        set_setting(&conn, "plex_url", "http://plex:32400/").unwrap(); // upsert
        assert_eq!(
            get_setting(&conn, "plex_url").unwrap().as_deref(),
            Some("http://plex:32400/")
        );
        delete_setting(&conn, "plex_url").unwrap();
        assert_eq!(get_setting(&conn, "plex_url").unwrap(), None);
    }

    #[test]
    fn authentik_settings_require_all_three_fields() {
        let pool = mem_pool();
        let conn = pool.get().unwrap();
        assert!(get_authentik_settings(&conn).unwrap().is_none());

        set_authentik_settings(&conn, "https://auth.example.com", "client-id", "secret").unwrap();
        let s = get_authentik_settings(&conn).unwrap().unwrap();
        assert_eq!(s.issuer_url, "https://auth.example.com");
        assert_eq!(s.client_id, "client-id");
        assert_eq!(s.client_secret, "secret");

        delete_authentik_settings(&conn).unwrap();
        assert!(get_authentik_settings(&conn).unwrap().is_none());
    }

    #[test]
    fn mappings_replace_skips_blank_and_preserves_order() {
        let pool = mem_pool();
        let mut conn = pool.get().unwrap();
        let input = vec![
            Mapping {
                plex_prefix: "/data/movies".into(),
                container_path: "/media/drive1".into(),
                label: Some("Movies".into()),
            },
            Mapping {
                plex_prefix: "   ".into(),
                container_path: "/media/drive2".into(),
                label: None,
            }, // skipped
            Mapping {
                plex_prefix: "/data/tv".into(),
                container_path: "/media/drive3".into(),
                label: None,
            },
        ];
        replace_mappings(&mut conn, &input).unwrap();
        let got = list_mappings(&conn).unwrap();
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].plex_prefix, "/data/movies");
        assert_eq!(got[0].label.as_deref(), Some("Movies"));
        assert_eq!(got[1].plex_prefix, "/data/tv");
        // Replacing again wipes the old rows.
        replace_mappings(&mut conn, &[]).unwrap();
        assert!(list_mappings(&conn).unwrap().is_empty());
    }
}
