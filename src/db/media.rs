//! Media rows: storing and looking up an indexed movie or episode.

use super::*;

/// A media item (movie or episode) we have indexed.
#[derive(Debug, Clone, Serialize)]
pub struct Media {
    pub id: i64,
    pub rating_key: String,
    pub kind: String, // "movie" | "episode"
    pub title: String,
    pub show: Option<String>,
    pub season: Option<i64>,
    pub episode: Option<i64>,
    pub year: Option<i64>,
    pub file_path: String,
    pub part_key: Option<String>,
    pub duration_ms: i64,
    pub access_mode: String,
}

/// Insert or replace a media row, returning its id. Clears any existing cues so
/// a reindex of the same item starts clean.
#[allow(clippy::too_many_arguments)]
pub fn upsert_media(
    conn: &rusqlite::Connection,
    rating_key: &str,
    kind: &str,
    title: &str,
    show: Option<&str>,
    season: Option<i64>,
    episode: Option<i64>,
    year: Option<i64>,
    file_path: &str,
    part_key: Option<&str>,
    duration_ms: i64,
    access_mode: &str,
    library: Option<&str>,
    file_mtime: i64,
    file_size: i64,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO media
            (rating_key, kind, title, show, season, episode, year, file_path,
             part_key, duration_ms, access_mode, library, file_mtime, file_size, indexed_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14, strftime('%s','now'))
         ON CONFLICT(rating_key) DO UPDATE SET
            kind=excluded.kind, title=excluded.title, show=excluded.show,
            season=excluded.season, episode=excluded.episode, year=excluded.year,
            file_path=excluded.file_path, part_key=excluded.part_key,
            duration_ms=excluded.duration_ms, access_mode=excluded.access_mode,
            library=excluded.library,
            file_mtime=excluded.file_mtime, file_size=excluded.file_size,
            indexed_at=strftime('%s','now')",
        params![
            rating_key,
            kind,
            title,
            show,
            season,
            episode,
            year,
            file_path,
            part_key,
            duration_ms,
            access_mode,
            library,
            file_mtime,
            file_size
        ],
    )?;
    let id: i64 = conn.query_row(
        "SELECT id FROM media WHERE rating_key = ?1",
        params![rating_key],
        |r| r.get(0),
    )?;
    conn.execute("DELETE FROM subtitle_cue WHERE media_id = ?1", params![id])?;
    conn.execute("UPDATE media SET cue_count = 0 WHERE id = ?1", params![id])?;
    Ok(id)
}

/// Return `(file_mtime, file_size, indexer_version)` for an already-indexed item,
/// if present. `indexer_version` lets the indexer tell "unchanged file, already
/// processed by the CURRENT indexing logic" apart from "unchanged file, but last
/// processed by an older version that didn't do sidecar lookup / library tagging
/// / etc." — so improving the indexer doesn't leave old rows stuck forever.
pub fn media_fingerprint(
    conn: &rusqlite::Connection,
    rating_key: &str,
) -> Result<Option<(i64, i64, i64)>> {
    let row = conn
        .query_row(
            "SELECT file_mtime, file_size, indexer_version FROM media WHERE rating_key = ?1",
            params![rating_key],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                ))
            },
        )
        .ok();
    Ok(row)
}

/// Stamp an item with the indexer logic version that produced its current data.
pub fn set_indexer_version(
    conn: &rusqlite::Connection,
    rating_key: &str,
    version: i64,
) -> Result<()> {
    conn.execute(
        "UPDATE media SET indexer_version = ?1 WHERE rating_key = ?2",
        params![version, rating_key],
    )?;
    Ok(())
}

pub fn get_media(conn: &rusqlite::Connection, rating_key: &str) -> Result<Option<Media>> {
    let m = conn
        .query_row(
            "SELECT id, rating_key, kind, title, show, season, episode, year,
                    file_path, part_key, duration_ms, access_mode
             FROM media WHERE rating_key = ?1",
            params![rating_key],
            row_to_media,
        )
        .ok();
    Ok(m)
}

pub(super) fn row_to_media(r: &rusqlite::Row) -> rusqlite::Result<Media> {
    Ok(Media {
        id: r.get(0)?,
        rating_key: r.get(1)?,
        kind: r.get(2)?,
        title: r.get(3)?,
        show: r.get(4)?,
        season: r.get(5)?,
        episode: r.get(6)?,
        year: r.get(7)?,
        file_path: r.get(8)?,
        part_key: r.get(9)?,
        duration_ms: r.get(10)?,
        access_mode: r.get(11)?,
    })
}

/// An already-indexed item rebuilt from its row, so it can be re-indexed
/// without asking the media server for it again.
pub fn media_item(
    conn: &rusqlite::Connection,
    rating_key: &str,
) -> Result<Option<crate::mediaserver::Item>> {
    let item = conn
        .query_row(
            "SELECT rating_key, kind, title, show, season, episode, year, file_path,
                    part_key, duration_ms, COALESCE(library, '')
             FROM media WHERE rating_key = ?1",
            params![rating_key],
            |r| {
                Ok(crate::mediaserver::Item {
                    rating_key: r.get(0)?,
                    kind: r.get(1)?,
                    title: r.get(2)?,
                    show: r.get(3)?,
                    season: r.get(4)?,
                    episode: r.get(5)?,
                    year: r.get(6)?,
                    file: Some(r.get(7)?),
                    part_key: r.get(8)?,
                    duration_ms: r.get(9)?,
                    library: r.get(10)?,
                })
            },
        )
        .optional()?;
    Ok(item)
}

/// How many subtitle lines an item has indexed.
pub fn cue_count_for(conn: &rusqlite::Connection, rating_key: &str) -> Result<i64> {
    Ok(conn.query_row(
        "SELECT COUNT(*) FROM subtitle_cue c JOIN media m ON m.id = c.media_id
         WHERE m.rating_key = ?1",
        params![rating_key],
        |r| r.get(0),
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::subtitles::Cue;

    fn cue(i: i64) -> Cue {
        Cue {
            start_ms: i * 1000,
            end_ms: i * 1000 + 500,
            text: format!("line {i}"),
        }
    }

    #[test]
    fn fingerprint_tracks_indexer_version() {
        let pool = mem_pool();
        let conn = pool.get().unwrap();
        assert!(media_fingerprint(&conn, "x").unwrap().is_none());

        upsert_media(
            &conn, "x", "movie", "X", None, None, None, None, "/x.mkv", None, 0, "direct", None,
            111, 222,
        )
        .unwrap();
        // Freshly upserted rows start at indexer_version 0 (not yet stamped).
        let (mtime, size, ver) = media_fingerprint(&conn, "x").unwrap().unwrap();
        assert_eq!((mtime, size, ver), (111, 222, 0));

        set_indexer_version(&conn, "x", 2).unwrap();
        let (_, _, ver) = media_fingerprint(&conn, "x").unwrap().unwrap();
        assert_eq!(ver, 2);
    }

    #[test]
    fn reindex_replaces_cues() {
        let pool = mem_pool();
        let mut conn = pool.get().unwrap();
        let id = upsert_media(
            &conn, "k", "movie", "X", None, None, None, None, "/x.mkv", None, 0, "direct", None, 1,
            1,
        )
        .unwrap();
        insert_cues(
            &mut conn,
            id,
            &[Cue {
                start_ms: 0,
                end_ms: 1,
                text: "old".into(),
            }],
        )
        .unwrap();
        // Re-upsert clears cues.
        let id2 = upsert_media(
            &conn, "k", "movie", "X", None, None, None, None, "/x.mkv", None, 0, "direct", None, 2,
            2,
        )
        .unwrap();
        assert_eq!(id, id2);
        insert_cues(
            &mut conn,
            id2,
            &[Cue {
                start_ms: 0,
                end_ms: 1,
                text: "new".into(),
            }],
        )
        .unwrap();
        assert!(search_dialogue(&conn, "old", 10).unwrap().is_empty());
        assert_eq!(search_dialogue(&conn, "new", 10).unwrap().len(), 1);
    }

    #[test]
    fn media_item_round_trips_what_the_indexer_needs() {
        let pool = mem_pool();
        let conn = pool.get().unwrap();
        add_media(
            &conn,
            "e9",
            "episode",
            "Pilot",
            Some("Show A"),
            "direct",
            "TV",
        );
        let item = media_item(&conn, "e9").unwrap().unwrap();
        assert_eq!(item.rating_key, "e9");
        assert_eq!(item.show.as_deref(), Some("Show A"));
        assert_eq!((item.season, item.episode), (Some(1), Some(2)));
        assert_eq!(item.file.as_deref(), Some("/m/e9.mkv"));
        assert_eq!(item.library, "TV");
        assert_eq!(item.duration_ms, 1000);
        assert!(media_item(&conn, "nope").unwrap().is_none());
    }

    #[test]
    fn cue_count_follows_inserts_and_reindexing() {
        let pool = mem_pool();
        let mut conn = pool.get().unwrap();
        let id = add_media(&conn, "k1", "movie", "A", None, "direct", "Movies");
        let count = |conn: &rusqlite::Connection| -> (i64, i64) {
            conn.query_row(
                "SELECT cue_count, (SELECT COUNT(*) FROM subtitle_cue WHERE media_id = ?1)
                 FROM media WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap()
        };
        assert_eq!(count(&conn), (0, 0));
        insert_cues(&mut conn, id, &[cue(1), cue(2), cue(3)]).unwrap();
        assert_eq!(count(&conn), (3, 3));
        insert_cues(&mut conn, id, &[cue(4)]).unwrap();
        assert_eq!(count(&conn), (4, 4));
        // re-indexing the item clears the cues, and the count with them
        add_media(&conn, "k1", "movie", "A", None, "direct", "Movies");
        assert_eq!(count(&conn), (0, 0));
        let stats = library_stats(&conn).unwrap();
        assert_eq!((stats[0].indexed, stats[0].no_subtitles), (0, 1));
    }

    #[test]
    fn databases_from_before_cue_count_get_it_filled_in() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE media (id INTEGER PRIMARY KEY, rating_key TEXT NOT NULL UNIQUE,
               kind TEXT NOT NULL, title TEXT NOT NULL, show TEXT, season INTEGER, episode INTEGER,
               year INTEGER, file_path TEXT NOT NULL, part_key TEXT, duration_ms INTEGER NOT NULL DEFAULT 0,
               access_mode TEXT NOT NULL, library TEXT, file_mtime INTEGER NOT NULL DEFAULT 0,
               file_size INTEGER NOT NULL DEFAULT 0, indexed_at INTEGER NOT NULL DEFAULT 0);
             CREATE TABLE subtitle_cue (id INTEGER PRIMARY KEY, media_id INTEGER NOT NULL,
               start_ms INTEGER NOT NULL, end_ms INTEGER NOT NULL, text TEXT NOT NULL);
             INSERT INTO media (id, rating_key, kind, title, file_path, access_mode)
               VALUES (1,'a','movie','A','/a','direct'), (2,'b','movie','B','/b','direct');
             INSERT INTO subtitle_cue (media_id, start_ms, end_ms, text)
               VALUES (1,0,1,'x'), (1,1,2,'y');",
        )
        .unwrap();
        migrate(&conn).unwrap();
        let counts: Vec<i64> = conn
            .prepare("SELECT cue_count FROM media ORDER BY id")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert_eq!(counts, [2, 0]);
        migrate(&conn).unwrap(); // and a second start leaves it alone
    }
}
