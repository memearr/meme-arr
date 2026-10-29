//! The subtitle-download requests the Requests tab follows.

use super::*;

/// A subtitle download the app asked the media server for, with where it
/// stands now.
#[derive(Debug, Clone, Serialize)]
pub struct SubtitleRequest {
    pub id: i64,
    pub rating_key: String,
    pub server: String,
    pub language: Option<String>,
    /// What was requested (the search result's title).
    pub subtitle: String,
    pub provider: Option<String>,
    pub requested_at: i64,
    /// `"indexed"`, `"waiting"` or `"item_missing"`; see [`request_state`].
    pub state: &'static str,
    /// Subtitle lines the item has indexed now.
    pub lines: i64,
    pub item_title: Option<String>,
    pub show: Option<String>,
    pub season: Option<i64>,
    pub episode: Option<i64>,
    pub year: Option<i64>,
    pub library: Option<String>,
}

/// How many requests are kept; older ones are dropped as new ones arrive.
pub(super) const MAX_SUBTITLE_REQUESTS: i64 = 500;

/// Where a request stands. A request counts as **indexed** once its item has
/// subtitle lines *and* was indexed at or after the moment of the request: an
/// item that merely already had lines (another language) doesn't count until
/// it has been re-processed since. Otherwise it's **waiting**: the server may
/// still be fetching the file, or saved it somewhere this app doesn't look.
pub fn request_state(
    item_exists: bool,
    lines: i64,
    indexed_at: i64,
    requested_at: i64,
) -> &'static str {
    if !item_exists {
        "item_missing"
    } else if lines > 0 && indexed_at >= requested_at {
        "indexed"
    } else {
        "waiting"
    }
}

pub fn add_subtitle_request(
    conn: &rusqlite::Connection,
    rating_key: &str,
    server: &str,
    language: Option<&str>,
    subtitle: &str,
    provider: Option<&str>,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO subtitle_request
            (rating_key, server, language, subtitle, provider, requested_at)
         VALUES (?1, ?2, ?3, ?4, ?5, strftime('%s','now'))",
        params![rating_key, server, language, subtitle, provider],
    )?;
    let id = conn.last_insert_rowid();
    conn.execute(
        "DELETE FROM subtitle_request WHERE id NOT IN
            (SELECT id FROM subtitle_request ORDER BY requested_at DESC, id DESC LIMIT ?1)",
        params![MAX_SUBTITLE_REQUESTS],
    )?;
    Ok(id)
}

/// Newest first, each joined to its item (for display) and given its state.
pub fn list_subtitle_requests(
    conn: &rusqlite::Connection,
    limit: i64,
) -> Result<Vec<SubtitleRequest>> {
    let mut stmt = conn.prepare(
        "SELECT r.id, r.rating_key, r.server, r.language, r.subtitle, r.provider, r.requested_at,
                m.id, m.title, m.show, m.season, m.episode, m.year, m.library, m.indexed_at,
                (SELECT COUNT(*) FROM subtitle_cue c WHERE c.media_id = m.id)
         FROM subtitle_request r
         LEFT JOIN media m ON m.rating_key = r.rating_key
         ORDER BY r.requested_at DESC, r.id DESC
         LIMIT ?1",
    )?;
    let rows = stmt.query_map(params![limit], |r| {
        let item_id: Option<i64> = r.get(7)?;
        let indexed_at: Option<i64> = r.get(14)?;
        let lines: Option<i64> = r.get(15)?;
        let requested_at: i64 = r.get(6)?;
        let lines = lines.unwrap_or(0);
        Ok(SubtitleRequest {
            id: r.get(0)?,
            rating_key: r.get(1)?,
            server: r.get(2)?,
            language: r.get(3)?,
            subtitle: r.get(4)?,
            provider: r.get(5)?,
            requested_at,
            state: request_state(
                item_id.is_some(),
                lines,
                indexed_at.unwrap_or(0),
                requested_at,
            ),
            lines,
            item_title: r.get(8)?,
            show: r.get(9)?,
            season: r.get(10)?,
            episode: r.get(11)?,
            year: r.get(12)?,
            library: r.get(13)?,
        })
    })?;
    Ok(rows.filter_map(Result::ok).collect())
}

/// The items someone asked the server to fetch a subtitle for. A scan reads
/// the server's own copy for these (see `remote_sub`) instead of for every item.
pub fn requested_rating_keys(
    conn: &rusqlite::Connection,
) -> Result<std::collections::HashSet<String>> {
    let mut stmt = conn.prepare("SELECT DISTINCT rating_key FROM subtitle_request")?;
    let keys = stmt
        .query_map([], |r| r.get::<_, String>(0))?
        .filter_map(Result::ok)
        .collect();
    Ok(keys)
}

/// Forget one request. Returns whether it existed.
pub fn delete_subtitle_request(conn: &rusqlite::Connection, id: i64) -> Result<bool> {
    Ok(conn.execute("DELETE FROM subtitle_request WHERE id = ?1", params![id])? > 0)
}

/// Forget every request that has become searchable subtitles; waiting ones stay.
pub fn clear_finished_subtitle_requests(conn: &rusqlite::Connection) -> Result<usize> {
    Ok(conn.execute(
        "DELETE FROM subtitle_request WHERE id IN (
            SELECT r.id FROM subtitle_request r
            JOIN media m ON m.rating_key = r.rating_key
            WHERE m.indexed_at >= r.requested_at
              AND EXISTS (SELECT 1 FROM subtitle_cue c WHERE c.media_id = m.id))",
        [],
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::subtitles::Cue;

    #[test]
    fn a_request_is_waiting_until_its_item_has_lines_from_an_index_pass_after_it() {
        assert_eq!(request_state(false, 0, 0, 100), "item_missing");
        assert_eq!(request_state(true, 0, 500, 100), "waiting"); // indexed, but nothing found
        assert_eq!(request_state(true, 12, 50, 100), "waiting"); // had lines, not re-indexed since
        assert_eq!(request_state(true, 12, 100, 100), "indexed"); // same second counts
        assert_eq!(request_state(true, 12, 101, 100), "indexed");
    }

    #[test]
    fn requests_list_newest_first_with_their_item_and_state() {
        let pool = mem_pool();
        let mut conn = pool.get().unwrap();
        let ep = add_media(
            &conn,
            "e1",
            "episode",
            "Pilot",
            Some("Show A"),
            "direct",
            "TV",
        );
        add_media(&conn, "m1", "movie", "Dune", None, "direct", "Movies");
        let first = add_subtitle_request(
            &conn,
            "e1",
            "plex",
            Some("en"),
            "Rel.WEB",
            Some("OpenSubtitles"),
        )
        .unwrap();
        // Make the first one older so the order doesn't depend on same-second ties.
        conn.execute(
            "UPDATE subtitle_request SET requested_at = requested_at - 50 WHERE id = ?1",
            params![first],
        )
        .unwrap();
        add_subtitle_request(&conn, "m1", "plex", None, "Dune.1080p", None).unwrap();
        add_subtitle_request(&conn, "gone", "plex", Some("fr"), "x", None).unwrap();

        let list = list_subtitle_requests(&conn, 50).unwrap();
        let keys: Vec<_> = list.iter().map(|r| r.rating_key.as_str()).collect();
        assert_eq!(keys.len(), 3);
        assert_eq!(*keys.last().unwrap(), "e1", "oldest last: {keys:?}");
        let by = |k: &str| list.iter().find(|r| r.rating_key == k).unwrap();
        assert_eq!(by("gone").state, "item_missing");
        assert_eq!(by("m1").state, "waiting");
        assert_eq!(by("e1").show.as_deref(), Some("Show A"));
        assert_eq!((by("e1").season, by("e1").episode), (Some(1), Some(2)));
        assert_eq!(by("e1").provider.as_deref(), Some("OpenSubtitles"));

        // The item gets subtitle lines from an index pass after the request.
        insert_cues(
            &mut conn,
            ep,
            &[Cue {
                start_ms: 0,
                end_ms: 1,
                text: "hi".into(),
            }],
        )
        .unwrap();
        conn.execute(
            "UPDATE media SET indexed_at = indexed_at + 10 WHERE rating_key = 'e1'",
            [],
        )
        .unwrap();
        let e1 = list_subtitle_requests(&conn, 50)
            .unwrap()
            .into_iter()
            .find(|r| r.rating_key == "e1")
            .unwrap();
        assert_eq!((e1.state, e1.lines), ("indexed", 1));
        // limit applies
        assert_eq!(list_subtitle_requests(&conn, 2).unwrap().len(), 2);
    }

    #[test]
    fn an_item_that_already_had_lines_is_not_indexed_until_it_is_reprocessed() {
        let pool = mem_pool();
        let mut conn = pool.get().unwrap();
        let id = add_media(&conn, "m", "movie", "M", None, "direct", "Movies");
        insert_cues(
            &mut conn,
            id,
            &[Cue {
                start_ms: 0,
                end_ms: 1,
                text: "english".into(),
            }],
        )
        .unwrap();
        conn.execute("UPDATE media SET indexed_at = indexed_at - 100", [])
            .unwrap(); // indexed earlier
        add_subtitle_request(&conn, "m", "jellyfin", Some("fr"), "French", None).unwrap();
        let r = &list_subtitle_requests(&conn, 5).unwrap()[0];
        assert_eq!((r.state, r.lines), ("waiting", 1));
    }

    #[test]
    fn requests_can_be_removed_and_finished_ones_cleared_while_waiting_ones_stay() {
        let pool = mem_pool();
        let mut conn = pool.get().unwrap();
        let done = add_media(&conn, "done", "movie", "Done", None, "direct", "Movies");
        insert_cues(
            &mut conn,
            done,
            &[Cue {
                start_ms: 0,
                end_ms: 1,
                text: "x".into(),
            }],
        )
        .unwrap();
        add_media(&conn, "wait", "movie", "Wait", None, "direct", "Movies");
        let a = add_subtitle_request(&conn, "done", "plex", None, "a", None).unwrap();
        let b = add_subtitle_request(&conn, "wait", "plex", None, "b", None).unwrap();

        assert_eq!(clear_finished_subtitle_requests(&conn).unwrap(), 1);
        let left: Vec<_> = list_subtitle_requests(&conn, 10)
            .unwrap()
            .into_iter()
            .map(|r| r.id)
            .collect();
        assert_eq!(left, vec![b]);
        assert!(!delete_subtitle_request(&conn, a).unwrap(), "already gone");
        assert!(delete_subtitle_request(&conn, b).unwrap());
        assert!(list_subtitle_requests(&conn, 10).unwrap().is_empty());
    }

    #[test]
    fn only_the_newest_requests_are_kept() {
        let pool = mem_pool();
        let conn = pool.get().unwrap();
        for i in 0..(MAX_SUBTITLE_REQUESTS + 5) {
            add_subtitle_request(&conn, &format!("k{i}"), "plex", None, "s", None).unwrap();
        }
        let list = list_subtitle_requests(&conn, 10_000).unwrap();
        assert_eq!(list.len() as i64, MAX_SUBTITLE_REQUESTS);
        let newest = format!("k{}", MAX_SUBTITLE_REQUESTS + 4);
        assert!(list.iter().any(|r| r.rating_key == newest));
        assert!(
            !list.iter().any(|r| r.rating_key == "k0"),
            "the oldest was dropped"
        );
    }
}
