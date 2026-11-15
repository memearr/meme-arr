//! The library, Wanted and browse queries: counts, missing subtitles, shows and episodes.

use super::*;

/// Per-library counts for the Make-page overview.
#[derive(Debug, Clone, Serialize, Default)]
pub struct LibraryStat {
    pub library: String,
    pub kind: String,
    pub total: i64,
    /// Items with at least one indexed subtitle cue.
    pub indexed: i64,
    /// Reachable on disk but no text subtitles were found (image-based subs, or none).
    pub no_subtitles: i64,
    /// Not reachable on a mounted volume (path mapping / mount issue).
    pub unreachable: i64,
    /// Total indexed subtitle lines across all items in this library+kind.
    pub cue_count: i64,
}

/// Count, per library + kind, how items break down: indexed vs. missing subtitles
/// vs. unreachable. `indexed + no_subtitles + unreachable == total`.
pub fn library_stats(conn: &rusqlite::Connection) -> Result<Vec<LibraryStat>> {
    let mut stmt = conn.prepare(
        "SELECT COALESCE(m.library, '(unknown)') AS lib, m.kind,
                COUNT(*) AS total,
                SUM(CASE WHEN m.cue_count > 0 THEN 1 ELSE 0 END) AS indexed,
                SUM(CASE WHEN m.cue_count = 0 AND m.access_mode = 'direct'
                         THEN 1 ELSE 0 END) AS no_subs,
                SUM(CASE WHEN m.access_mode != 'direct' THEN 1 ELSE 0 END) AS unreachable,
                SUM(m.cue_count) AS cue_count
         FROM media m
         GROUP BY lib, m.kind
         ORDER BY lib, m.kind",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(LibraryStat {
            library: r.get(0)?,
            kind: r.get(1)?,
            total: r.get(2)?,
            indexed: r.get(3)?,
            no_subtitles: r.get(4)?,
            unreachable: r.get(5)?,
            cue_count: r.get(6)?,
        })
    })?;
    Ok(rows.filter_map(Result::ok).collect())
}

/// Remove indexed rows for any library NOT in `mapped_libraries` (and their
/// cues, via the FK cascade). Called at the start of a reindex so that a
/// library whose mapping was deleted — or never configured — stops cluttering
/// stats with stale data instead of lingering forever. Rows with no library tag
/// yet (pre-migration data) are left alone; they get tagged on their next index
/// pass and pruned then if still unmapped.
pub fn prune_unmapped_libraries(
    conn: &rusqlite::Connection,
    mapped_libraries: &[String],
) -> Result<usize> {
    if mapped_libraries.is_empty() {
        // No mappings configured at all: nothing is "mapped", but we still only
        // prune rows that DO have a library tag — never touch (unknown) rows.
        return Ok(conn.execute("DELETE FROM media WHERE library IS NOT NULL", [])?);
    }
    let placeholders = mapped_libraries
        .iter()
        .map(|_| "?")
        .collect::<Vec<_>>()
        .join(",");
    let sql =
        format!("DELETE FROM media WHERE library IS NOT NULL AND library NOT IN ({placeholders})");
    let params: Vec<&dyn rusqlite::ToSql> = mapped_libraries
        .iter()
        .map(|s| s as &dyn rusqlite::ToSql)
        .collect();
    Ok(conn.execute(&sql, params.as_slice())?)
}

/// An item that failed to index, with the reason.
#[derive(Debug, Clone, Serialize)]
pub struct MissingItem {
    pub rating_key: String,
    pub kind: String,
    pub title: String,
    pub show: Option<String>,
    pub season: Option<i64>,
    pub episode: Option<i64>,
    pub file_path: String,
    pub reason: String, // "no_subtitles" | "unreachable"
}

/// List items in a library that have no indexed cues, with why.
pub fn missing_items(
    conn: &rusqlite::Connection,
    library: &str,
    limit: i64,
) -> Result<Vec<MissingItem>> {
    let mut stmt = conn.prepare(
        "SELECT m.rating_key, m.kind, m.title, m.show, m.season, m.episode,
                m.file_path, m.access_mode
         FROM media m
         WHERE COALESCE(m.library, '(unknown)') = ?1 AND m.cue_count = 0
         ORDER BY m.show, m.season, m.episode, m.title
         LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![library, limit], |r| {
        let access: String = r.get(7)?;
        Ok(MissingItem {
            rating_key: r.get(0)?,
            kind: r.get(1)?,
            title: r.get(2)?,
            show: r.get(3)?,
            season: r.get(4)?,
            episode: r.get(5)?,
            file_path: r.get(6)?,
            reason: if access == "direct" {
                "no_subtitles".into()
            } else {
                "unreachable".into()
            },
        })
    })?;
    Ok(rows.filter_map(Result::ok).collect())
}

/// An item that has no indexed subtitles but whose file *is* reachable, so
/// finding a subtitle for it would actually help (Sonarr's "Wanted").
#[derive(Debug, Clone, Serialize)]
pub struct WantedItem {
    pub rating_key: String,
    pub kind: String,
    pub title: String,
    pub show: Option<String>,
    pub season: Option<i64>,
    pub episode: Option<i64>,
    pub year: Option<i64>,
    pub library: Option<String>,
}

/// One page of wanted items (optionally filtered by text and/or library),
/// the total, and how many *unreachable* items are excluded -- those need a
/// path mapping fixed, not a subtitle.
pub fn wanted_items(
    conn: &rusqlite::Connection,
    query: &str,
    libraries: &[String],
    limit: i64,
    offset: i64,
) -> Result<(Vec<WantedItem>, i64, i64)> {
    let like = format!("%{}%", query.replace(['%', '_'], ""));
    let no_cues = "NOT EXISTS (SELECT 1 FROM subtitle_cue c WHERE c.media_id = m.id)";
    let (lib_sql, lib_vals) = library_clause("m.library", libraries, 2);
    let filter = format!("(m.title LIKE ?1 OR m.show LIKE ?1){lib_sql}");
    let binds = || -> Vec<rusqlite::types::Value> {
        let mut v: Vec<rusqlite::types::Value> = vec![like.clone().into()];
        v.extend(lib_vals.iter().cloned());
        v
    };

    let total: i64 = conn.query_row(
        &format!("SELECT COUNT(*) FROM media m WHERE m.access_mode = 'direct' AND {no_cues} AND {filter}"),
        rusqlite::params_from_iter(binds()),
        |r| r.get(0),
    )?;
    let unreachable: i64 = conn.query_row(
        &format!("SELECT COUNT(*) FROM media m WHERE m.access_mode != 'direct' AND {no_cues} AND {filter}"),
        rusqlite::params_from_iter(binds()),
        |r| r.get(0),
    )?;
    let (limit_n, offset_n) = (2 + libraries.len(), 3 + libraries.len());
    let mut stmt = conn.prepare(&format!(
        "SELECT m.rating_key, m.kind, m.title, m.show, m.season, m.episode, m.year, m.library
         FROM media m
         WHERE m.access_mode = 'direct' AND {no_cues} AND {filter}
         ORDER BY COALESCE(m.show, m.title), m.season, m.episode, m.title
         LIMIT ?{limit_n} OFFSET ?{offset_n}"
    ))?;
    let mut page_binds = binds();
    page_binds.push(limit.into());
    page_binds.push(offset.into());
    let rows = stmt.query_map(rusqlite::params_from_iter(page_binds), |r| {
        Ok(WantedItem {
            rating_key: r.get(0)?,
            kind: r.get(1)?,
            title: r.get(2)?,
            show: r.get(3)?,
            season: r.get(4)?,
            episode: r.get(5)?,
            year: r.get(6)?,
            library: r.get(7)?,
        })
    })?;
    Ok((rows.filter_map(Result::ok).collect(), total, unreachable))
}

/// A Library-tab entry: one row per movie, one row per show (episodes collapsed).
#[derive(Debug, Clone, Serialize)]
pub struct BrowseEntry {
    pub kind: String, // "movie" | "show"
    pub name: String,
    pub show: Option<String>,
    /// Set for movies (so the UI can open them directly); null for shows.
    pub rating_key: Option<String>,
    pub count: i64,   // movies: 1; shows: number of episodes
    pub indexed: i64, // how many of those have subtitle cues
    pub year: Option<i64>,
}

/// Which entries the library browse lists, by how many of their items have subtitles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BrowseStatus {
    #[default]
    All,
    /// At least one item (movie, or episode of a show) has no indexed subtitles.
    Missing,
    /// Every item has indexed subtitles.
    Indexed,
}

impl BrowseStatus {
    pub fn parse(s: &str) -> Self {
        match s {
            "missing" => Self::Missing,
            "indexed" => Self::Indexed,
            _ => Self::All,
        }
    }
}

/// Browse the library grouped: movies individually, episodes collapsed by show.
/// Returns a page of entries plus the total number of entries (for paging).
/// `libraries` limits results to some Plex libraries (none = the whole
/// catalog), and `status` to entries with or without missing subtitles.
pub fn browse_library(
    conn: &rusqlite::Connection,
    query: &str,
    libraries: &[String],
    status: BrowseStatus,
    limit: i64,
    offset: i64,
) -> Result<(Vec<BrowseEntry>, i64)> {
    let like = format!("%{}%", query.replace(['%', '_'], ""));
    // Group key: movies by rating_key (one each), episodes by show.
    let group =
        "CASE WHEN m.kind='episode' THEN 'S:'||COALESCE(m.show,'') ELSE 'M:'||m.rating_key END";
    let name = "CASE WHEN m.kind='episode' THEN m.show ELSE m.title END";
    // Match the movie title, the show name, OR an individual episode's own
    // title — so searching an episode title (e.g. "Tideline") still
    // surfaces its show (episodes are grouped under their show in this view).
    let (lib_sql, lib_vals) = library_clause("m.library", libraries, 2);
    let matches = format!("({name} LIKE ?1 OR (m.kind='episode' AND m.title LIKE ?1)){lib_sql}");
    // Per-item cue counts come from `media.cue_count`: counting them with a
    // GROUP BY over subtitle_cue reads the whole (huge) table, which took
    // seconds on a big library.
    let indexed_n = "SUM(CASE WHEN m.cue_count > 0 THEN 1 ELSE 0 END)";
    let having = match status {
        BrowseStatus::All => String::new(),
        BrowseStatus::Missing => format!(" HAVING {indexed_n} < COUNT(*)"),
        BrowseStatus::Indexed => format!(" HAVING {indexed_n} = COUNT(*)"),
    };

    let mut binds: Vec<rusqlite::types::Value> = vec![like.into()];
    binds.extend(lib_vals);
    let total: i64 = conn.query_row(
        &format!(
            "SELECT COUNT(*) FROM (
                SELECT 1 FROM media m
                WHERE {matches} GROUP BY {group}{having}
             )"
        ),
        rusqlite::params_from_iter(binds.iter()),
        |r| r.get(0),
    )?;

    let sql = format!(
        "SELECT CASE WHEN m.kind='episode' THEN 'show' ELSE 'movie' END AS ekind,
                {name} AS name,
                CASE WHEN m.kind='episode' THEN m.show ELSE NULL END AS show,
                CASE WHEN m.kind='episode' THEN NULL ELSE m.rating_key END AS rk,
                COUNT(*) AS cnt,
                {indexed_n} AS indexed,
                MIN(m.year) AS year
         FROM media m
         WHERE {matches}
         GROUP BY {group}{having}
         ORDER BY name
         LIMIT ?{} OFFSET ?{}",
        binds.len() + 1,
        binds.len() + 2
    );
    binds.push(limit.into());
    binds.push(offset.into());
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(binds.iter()), |r| {
        Ok(BrowseEntry {
            kind: r.get(0)?,
            name: r.get::<_, Option<String>>(1)?.unwrap_or_default(),
            show: r.get(2)?,
            rating_key: r.get(3)?,
            count: r.get(4)?,
            indexed: r.get(5)?,
            year: r.get(6)?,
        })
    })?;
    Ok((rows.filter_map(Result::ok).collect(), total))
}

/// An episode row for the show drill-down.
#[derive(Debug, Clone, Serialize)]
pub struct EpisodeRow {
    pub rating_key: String,
    pub title: String,
    pub show: Option<String>,
    pub season: Option<i64>,
    pub episode: Option<i64>,
    pub indexed: bool,
}

/// Episodes of one show, ordered, with whether each has subtitles.
pub fn episodes_in_show(
    conn: &rusqlite::Connection,
    show: &str,
    limit: i64,
) -> Result<Vec<EpisodeRow>> {
    let mut stmt = conn.prepare(
        "SELECT m.rating_key, m.title, m.show, m.season, m.episode,
                CASE WHEN m.cue_count > 0 THEN 1 ELSE 0 END AS indexed
         FROM media m
         WHERE m.kind = 'episode' AND COALESCE(m.show,'') = ?1
         ORDER BY m.season, m.episode, m.title
         LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![show, limit], |r| {
        Ok(EpisodeRow {
            rating_key: r.get(0)?,
            title: r.get(1)?,
            show: r.get(2)?,
            season: r.get(3)?,
            episode: r.get(4)?,
            indexed: r.get::<_, i64>(5)? > 0,
        })
    })?;
    Ok(rows.filter_map(Result::ok).collect())
}

/// Distinct show names matching `query` — lets a dialogue/title search also
/// surface "this looks like a show name" as a selectable option alongside
/// the regular results, so searching "The Lantern Cove" finds the show even if
/// those words never occur in a line of dialogue.
pub fn matching_shows(conn: &rusqlite::Connection, query: &str, limit: i64) -> Result<Vec<String>> {
    matching_shows_in(conn, query, Scope::default(), limit)
}

/// Show names matching `query`, limited to some libraries (none = all).
pub fn matching_shows_in(
    conn: &rusqlite::Connection,
    query: &str,
    scope: Scope,
    limit: i64,
) -> Result<Vec<String>> {
    if query.trim().is_empty() {
        return Ok(Vec::new());
    }
    let like = like_pattern(query);
    let (lib_sql, lib_vals) = scope_clause(&scope, 3);
    let mut stmt = conn.prepare(&format!(
        "SELECT DISTINCT m.show FROM media m
         WHERE m.kind = 'episode' AND m.show IS NOT NULL AND m.show LIKE ?1{lib_sql}
         ORDER BY m.show
         LIMIT ?2"
    ))?;
    let mut binds: Vec<rusqlite::types::Value> = vec![like.into(), limit.into()];
    binds.extend(lib_vals);
    let rows = stmt.query_map(rusqlite::params_from_iter(binds), |r| r.get::<_, String>(0))?;
    Ok(rows.filter_map(Result::ok).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::subtitles::Cue;

    #[test]
    fn library_stats_breaks_down_by_reason() {
        let pool = mem_pool();
        let mut conn = pool.get().unwrap();
        // indexed (direct + cues)
        let a = upsert_media(
            &conn,
            &MediaUpsert {
                rating_key: "a",
                kind: "movie",
                title: "A",
                show: None,
                season: None,
                episode: None,
                year: None,
                file_path: "/m/a.mkv",
                part_key: None,
                duration_ms: 0,
                access_mode: "direct",
                library: Some("Movies"),
                file_mtime: 1,
                file_size: 1,
            },
        )
        .unwrap();
        insert_cues(
            &mut conn,
            a,
            &[Cue {
                start_ms: 0,
                end_ms: 1,
                text: "hi".into(),
            }],
        )
        .unwrap();
        // direct but no subtitles
        upsert_media(
            &conn,
            &MediaUpsert {
                rating_key: "b",
                kind: "movie",
                title: "B",
                show: None,
                season: None,
                episode: None,
                year: None,
                file_path: "/m/b.mkv",
                part_key: None,
                duration_ms: 0,
                access_mode: "direct",
                library: Some("Movies"),
                file_mtime: 1,
                file_size: 1,
            },
        )
        .unwrap();
        // unreachable (download mode)
        upsert_media(
            &conn,
            &MediaUpsert {
                rating_key: "c",
                kind: "movie",
                title: "C",
                show: None,
                season: None,
                episode: None,
                year: None,
                file_path: "D:\\x.mkv",
                part_key: None,
                duration_ms: 0,
                access_mode: "plex-download",
                library: Some("Movies"),
                file_mtime: 0,
                file_size: 0,
            },
        )
        .unwrap();

        let stats = library_stats(&conn).unwrap();
        assert_eq!(stats.len(), 1);
        let s = &stats[0];
        assert_eq!(s.library, "Movies");
        assert_eq!(s.total, 3);
        assert_eq!(s.indexed, 1);
        assert_eq!(s.no_subtitles, 1);
        assert_eq!(s.unreachable, 1);
        assert_eq!(s.cue_count, 1);

        let missing = missing_items(&conn, "Movies", 10).unwrap();
        assert_eq!(missing.len(), 2);
        let reasons: Vec<_> = missing.iter().map(|m| m.reason.as_str()).collect();
        assert!(reasons.contains(&"no_subtitles"));
        assert!(reasons.contains(&"unreachable"));
    }

    #[test]
    fn browse_groups_shows_and_lists_movies() {
        let pool = mem_pool();
        let mut conn = pool.get().unwrap();
        // One movie, one show with two episodes (one indexed).
        let mv = upsert_media(
            &conn,
            &MediaUpsert {
                rating_key: "m1",
                kind: "movie",
                title: "Paper Moon Heist",
                show: None,
                season: None,
                episode: None,
                year: Some(2021),
                file_path: "/m/heist.mkv",
                part_key: None,
                duration_ms: 0,
                access_mode: "direct",
                library: Some("Movies"),
                file_mtime: 1,
                file_size: 1,
            },
        )
        .unwrap();
        insert_cues(
            &mut conn,
            mv,
            &[Cue {
                start_ms: 0,
                end_ms: 1,
                text: "spice".into(),
            }],
        )
        .unwrap();
        let e1 = upsert_media(
            &conn,
            &MediaUpsert {
                rating_key: "e1",
                kind: "episode",
                title: "Pilot",
                show: Some("BB"),
                season: Some(1),
                episode: Some(1),
                year: Some(2008),
                file_path: "/m/bb1.mkv",
                part_key: None,
                duration_ms: 0,
                access_mode: "direct",
                library: Some("TV"),
                file_mtime: 1,
                file_size: 1,
            },
        )
        .unwrap();
        insert_cues(
            &mut conn,
            e1,
            &[Cue {
                start_ms: 0,
                end_ms: 1,
                text: "knock".into(),
            }],
        )
        .unwrap();
        upsert_media(
            &conn,
            &MediaUpsert {
                rating_key: "e2",
                kind: "episode",
                title: "Cat's in the Bag",
                show: Some("BB"),
                season: Some(1),
                episode: Some(2),
                year: Some(2008),
                file_path: "/m/bb2.mkv",
                part_key: None,
                duration_ms: 0,
                access_mode: "direct",
                library: Some("TV"),
                file_mtime: 1,
                file_size: 1,
            },
        )
        .unwrap();

        let (entries, total) = browse_library(&conn, "", &[], BrowseStatus::All, 50, 0).unwrap();
        assert_eq!(total, 2); // one movie entry + one show entry
        assert_eq!(entries.len(), 2);
        let show = entries.iter().find(|e| e.kind == "show").unwrap();
        assert_eq!(show.name, "BB");
        assert_eq!(show.count, 2);
        assert_eq!(show.indexed, 1);
        let movie = entries.iter().find(|e| e.kind == "movie").unwrap();
        assert_eq!(movie.rating_key.as_deref(), Some("m1"));
        assert_eq!(movie.count, 1);

        let eps = episodes_in_show(&conn, "BB", 50).unwrap();
        assert_eq!(eps.len(), 2);
        assert_eq!(eps[0].episode, Some(1));
        assert!(eps[0].indexed);
        assert!(!eps[1].indexed);
    }

    #[test]
    fn browse_library_search_matches_movie_show_and_episode_titles() {
        let pool = mem_pool();
        let conn = pool.get().unwrap();
        upsert_media(
            &conn,
            &MediaUpsert {
                rating_key: "m1",
                kind: "movie",
                title: "Paper Moon Heist",
                show: None,
                season: None,
                episode: None,
                year: Some(2021),
                file_path: "/m/heist.mkv",
                part_key: None,
                duration_ms: 0,
                access_mode: "direct",
                library: Some("Movies"),
                file_mtime: 1,
                file_size: 1,
            },
        )
        .unwrap();
        upsert_media(
            &conn,
            &MediaUpsert {
                rating_key: "e1",
                kind: "episode",
                title: "Pilot",
                show: Some("The Lantern Cove"),
                season: Some(1),
                episode: Some(1),
                year: Some(2008),
                file_path: "/m/bb1.mkv",
                part_key: None,
                duration_ms: 0,
                access_mode: "direct",
                library: Some("TV"),
                file_mtime: 1,
                file_size: 1,
            },
        )
        .unwrap();
        upsert_media(
            &conn,
            &MediaUpsert {
                rating_key: "e2",
                kind: "episode",
                title: "Tideline",
                show: Some("The Lantern Cove"),
                season: Some(5),
                episode: Some(14),
                year: Some(2013),
                file_path: "/m/bb2.mkv",
                part_key: None,
                duration_ms: 0,
                access_mode: "direct",
                library: Some("TV"),
                file_mtime: 1,
                file_size: 1,
            },
        )
        .unwrap();

        // Movie title matches directly.
        let (entries, total) =
            browse_library(&conn, "Paper Moon Heist", &[], BrowseStatus::All, 50, 0).unwrap();
        assert_eq!(total, 1);
        assert_eq!(entries[0].kind, "movie");

        // Show name matches directly.
        let (entries, total) =
            browse_library(&conn, "The Lantern Cove", &[], BrowseStatus::All, 50, 0).unwrap();
        assert_eq!(total, 1);
        assert_eq!(entries[0].kind, "show");

        // An individual EPISODE's own title must also surface its show — this
        // is the gap that was fixed: searching "Tideline" previously found
        // nothing, because only the show name was matched for episode rows.
        let (entries, total) =
            browse_library(&conn, "Tideline", &[], BrowseStatus::All, 50, 0).unwrap();
        assert_eq!(total, 1, "episode title search should find the parent show");
        assert_eq!(entries[0].kind, "show");
        assert_eq!(entries[0].name, "The Lantern Cove");

        // A non-matching query finds nothing.
        let (entries, total) =
            browse_library(&conn, "nonexistent", &[], BrowseStatus::All, 50, 0).unwrap();
        assert_eq!(total, 0);
        assert!(entries.is_empty());
    }

    #[test]
    fn browse_library_scopes_to_one_library_when_given() {
        let pool = mem_pool();
        let conn = pool.get().unwrap();
        upsert_media(
            &conn,
            &MediaUpsert {
                rating_key: "m1",
                kind: "movie",
                title: "Paper Moon Heist",
                show: None,
                season: None,
                episode: None,
                year: Some(2021),
                file_path: "/m/heist.mkv",
                part_key: None,
                duration_ms: 0,
                access_mode: "direct",
                library: Some("Movies"),
                file_mtime: 1,
                file_size: 1,
            },
        )
        .unwrap();
        upsert_media(
            &conn,
            &MediaUpsert {
                rating_key: "e1",
                kind: "episode",
                title: "Pilot",
                show: Some("The Lantern Cove"),
                season: Some(1),
                episode: Some(1),
                year: Some(2008),
                file_path: "/m/bb1.mkv",
                part_key: None,
                duration_ms: 0,
                access_mode: "direct",
                library: Some("TV"),
                file_mtime: 1,
                file_size: 1,
            },
        )
        .unwrap();

        // No library filter: both the movie and the show are visible.
        let (entries, total) = browse_library(&conn, "", &[], BrowseStatus::All, 50, 0).unwrap();
        assert_eq!(total, 2);
        assert_eq!(entries.len(), 2);

        // Scoped to "Movies": only Paper Moon Heist.
        let (entries, total) =
            browse_library(&conn, "", &["Movies".to_string()], BrowseStatus::All, 50, 0).unwrap();
        assert_eq!(total, 1);
        assert_eq!(entries[0].name, "Paper Moon Heist");

        // Scoped to "TV": only The Lantern Cove.
        let (entries, total) =
            browse_library(&conn, "", &["TV".to_string()], BrowseStatus::All, 50, 0).unwrap();
        assert_eq!(total, 1);
        assert_eq!(entries[0].name, "The Lantern Cove");

        // Scoped to a library with nothing in it: empty, not an error.
        let (entries, total) =
            browse_library(&conn, "", &["Music".to_string()], BrowseStatus::All, 50, 0).unwrap();
        assert_eq!(total, 0);
        assert!(entries.is_empty());
    }

    #[test]
    fn matching_shows_finds_distinct_show_names_only() {
        let pool = mem_pool();
        let conn = pool.get().unwrap();
        upsert_media(
            &conn,
            &MediaUpsert {
                rating_key: "m1",
                kind: "movie",
                title: "Lantern Point",
                show: None,
                season: None,
                episode: None,
                year: Some(2019),
                file_path: "/m/bp.mkv",
                part_key: None,
                duration_ms: 0,
                access_mode: "direct",
                library: Some("Movies"),
                file_mtime: 1,
                file_size: 1,
            },
        )
        .unwrap();
        upsert_media(
            &conn,
            &MediaUpsert {
                rating_key: "e1",
                kind: "episode",
                title: "Pilot",
                show: Some("The Lantern Cove"),
                season: Some(1),
                episode: Some(1),
                year: Some(2008),
                file_path: "/m/bb1.mkv",
                part_key: None,
                duration_ms: 0,
                access_mode: "direct",
                library: Some("TV"),
                file_mtime: 1,
                file_size: 1,
            },
        )
        .unwrap();
        upsert_media(
            &conn,
            &MediaUpsert {
                rating_key: "e2",
                kind: "episode",
                title: "Tideline",
                show: Some("The Lantern Cove"),
                season: Some(5),
                episode: Some(14),
                year: Some(2013),
                file_path: "/m/bb2.mkv",
                part_key: None,
                duration_ms: 0,
                access_mode: "direct",
                library: Some("TV"),
                file_mtime: 1,
                file_size: 1,
            },
        )
        .unwrap();

        // Matches the show, deduplicated across its two episodes -- and not
        // the movie, even though its title also contains "Lantern".
        let shows = matching_shows(&conn, "Lantern", 50).unwrap();
        assert_eq!(shows, vec!["The Lantern Cove".to_string()]);

        // An episode's own title doesn't match a show search (that's
        // `browse_library`'s job, which deliberately also checks episode
        // titles) -- this function is specifically "does the query look
        // like a show name."
        assert!(matching_shows(&conn, "Tideline", 50).unwrap().is_empty());

        assert!(matching_shows(&conn, "nonexistent", 50).unwrap().is_empty());
        assert!(matching_shows(&conn, "", 50).unwrap().is_empty());
        assert!(matching_shows(&conn, "   ", 50).unwrap().is_empty());
    }

    #[test]
    fn prune_unmapped_libraries_removes_only_unmapped_tagged_rows() {
        let pool = mem_pool();
        let conn = pool.get().unwrap();
        upsert_media(
            &conn,
            &MediaUpsert {
                rating_key: "a",
                kind: "movie",
                title: "A",
                show: None,
                season: None,
                episode: None,
                year: None,
                file_path: "/a.mkv",
                part_key: None,
                duration_ms: 0,
                access_mode: "direct",
                library: Some("Movies"),
                file_mtime: 1,
                file_size: 1,
            },
        )
        .unwrap();
        upsert_media(
            &conn,
            &MediaUpsert {
                rating_key: "b",
                kind: "movie",
                title: "B",
                show: None,
                season: None,
                episode: None,
                year: None,
                file_path: "/b.mkv",
                part_key: None,
                duration_ms: 0,
                access_mode: "direct",
                library: Some("Anime"),
                file_mtime: 1,
                file_size: 1,
            },
        )
        .unwrap();
        upsert_media(
            &conn,
            &MediaUpsert {
                rating_key: "c",
                kind: "movie",
                title: "C",
                show: None,
                season: None,
                episode: None,
                year: None,
                file_path: "/c.mkv",
                part_key: None,
                duration_ms: 0,
                access_mode: "direct",
                library: None,
                file_mtime: 1,
                file_size: 1,
            },
        )
        .unwrap(); // legacy, no library tag

        let removed = prune_unmapped_libraries(&conn, &["Movies".to_string()]).unwrap();
        assert_eq!(removed, 1); // only "Anime" (tagged + unmapped)
        assert!(get_media(&conn, "a").unwrap().is_some()); // Movies: kept
        assert!(get_media(&conn, "b").unwrap().is_none()); // Anime: pruned
        assert!(get_media(&conn, "c").unwrap().is_some()); // untagged legacy: kept

        // With no mappings at all, every tagged row is pruned but untagged ones survive.
        let removed2 = prune_unmapped_libraries(&conn, &[]).unwrap();
        assert_eq!(removed2, 1); // "Movies" row
        assert!(get_media(&conn, "a").unwrap().is_none());
        assert!(get_media(&conn, "c").unwrap().is_some());
    }

    #[test]
    fn wanted_lists_only_reachable_items_without_subtitles() {
        let pool = mem_pool();
        let mut conn = pool.get().unwrap();
        let has = add_media(&conn, "has", "movie", "Has Subs", None, "direct", "Movies");
        insert_cues(
            &mut conn,
            has,
            &[Cue {
                start_ms: 0,
                end_ms: 1,
                text: "hi".into(),
            }],
        )
        .unwrap();
        add_media(
            &conn,
            "none1",
            "movie",
            "Needs Subs",
            None,
            "direct",
            "Movies",
        );
        add_media(
            &conn,
            "ep1",
            "episode",
            "Pilot",
            Some("Show A"),
            "direct",
            "TV",
        );
        add_media(
            &conn,
            "gone",
            "movie",
            "Unmounted",
            None,
            "plex-download",
            "Movies",
        );

        let (items, total, unreachable) = wanted_items(&conn, "", &[], 50, 0).unwrap();
        let keys: Vec<&str> = items.iter().map(|i| i.rating_key.as_str()).collect();
        assert_eq!(keys, ["none1", "ep1"]);
        assert_eq!(total, 2);
        assert_eq!(unreachable, 1, "unreachable items are counted, not listed");

        // Text filter matches a show name as well as a title.
        let (items, total, _) = wanted_items(&conn, "show a", &[], 50, 0).unwrap();
        assert_eq!((items.len(), total), (1, 1));
        // Library scope.
        let (items, _, unreachable) =
            wanted_items(&conn, "", &["Movies".to_string()], 50, 0).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(unreachable, 1);
        // Paging: total stays the full count while the page is smaller.
        let (page, total, _) = wanted_items(&conn, "", &[], 1, 1).unwrap();
        assert_eq!((page.len(), total), (1, 2));
        assert_eq!(page[0].rating_key, "ep1");
    }

    #[test]
    fn an_item_gaining_subtitles_leaves_the_wanted_list() {
        let pool = mem_pool();
        let mut conn = pool.get().unwrap();
        let id = add_media(&conn, "m", "movie", "M", None, "direct", "Movies");
        assert_eq!(wanted_items(&conn, "", &[], 10, 0).unwrap().1, 1);
        assert_eq!(cue_count_for(&conn, "m").unwrap(), 0);
        insert_cues(
            &mut conn,
            id,
            &[Cue {
                start_ms: 0,
                end_ms: 1,
                text: "x".into(),
            }],
        )
        .unwrap();
        assert_eq!(wanted_items(&conn, "", &[], 10, 0).unwrap().1, 0);
        assert_eq!(cue_count_for(&conn, "m").unwrap(), 1);
    }

    #[test]
    fn browse_filters_by_several_libraries_and_by_missing_subtitles() {
        let pool = mem_pool();
        let mut conn = pool.get().unwrap();
        seed_libraries(&mut conn, 2); // every item of TV, Anime, Movies has cues
                                      // a show with one episode lacking subtitles, and a movie lacking them
        add_media(
            &conn,
            "tv-gap",
            "episode",
            "Gap",
            Some("TV Show"),
            "direct",
            "TV",
        );
        add_media(&conn, "mv-bare", "movie", "Bare", None, "direct", "Movies");
        let list = |libs: &[&str], st| -> Vec<String> {
            let (e, total) = browse_library(&conn, "", &names(libs), st, 50, 0).unwrap();
            assert_eq!(total as usize, e.len());
            e.into_iter().map(|e| e.name).collect()
        };
        use BrowseStatus::*;
        assert_eq!(list(&[], All).len(), 2 + 3); // TV Show, Anime Show, 3 movies
        assert_eq!(list(&["Movies"], All).len(), 3);
        assert_eq!(list(&["TV", "Anime"], All), ["Anime Show", "TV Show"]);
        // missing: the show with a gap and the bare movie
        assert_eq!(list(&[], Missing), ["Bare", "TV Show"]);
        assert_eq!(list(&["TV"], Missing), ["TV Show"]);
        assert_eq!(list(&["Anime"], Missing), Vec::<String>::new());
        // fully indexed: everything else
        assert_eq!(list(&[], Indexed).len(), 3); // Anime Show + the 2 seeded movies
        assert!(!list(&[], Indexed).contains(&"TV Show".to_string()));
    }

    #[test]
    fn matching_shows_respect_the_library_selection() {
        let pool = mem_pool();
        let mut conn = pool.get().unwrap();
        seed_libraries(&mut conn, 1);
        assert_eq!(
            matching_shows_in(&conn, "Show", Scope::default(), 10).unwrap(),
            ["Anime Show", "TV Show"]
        );
        assert_eq!(
            matching_shows_in(&conn, "Show", Scope::libraries(&names(&["Anime"])), 10).unwrap(),
            ["Anime Show"]
        );
    }

    #[test]
    fn wanted_can_be_limited_to_several_libraries() {
        let pool = mem_pool();
        let conn = pool.get().unwrap();
        for lib in ["TV", "Anime", "Movies"] {
            add_media(
                &conn,
                &format!("{lib}-1"),
                "movie",
                lib,
                None,
                "direct",
                lib,
            );
        }
        let (all, total, _) = wanted_items(&conn, "", &[], 50, 0).unwrap();
        assert_eq!((all.len(), total), (3, 3));
        let (two, total, _) = wanted_items(&conn, "", &names(&["TV", "Anime"]), 50, 0).unwrap();
        assert_eq!((two.len(), total), (2, 2));
        let (page, total, _) = wanted_items(&conn, "", &names(&["TV", "Anime"]), 1, 1).unwrap();
        assert_eq!((page.len(), total), (1, 2), "paging works with a filter");
    }

    #[test]
    fn show_suggestions_follow_the_operators() {
        let pool = mem_pool();
        let mut conn = pool.get().unwrap();
        seed_libraries(&mut conn, 1);
        let parsed = crate::querysyntax::parse("show -show:anime");
        let scope = Scope {
            libraries: &[],
            filters: &parsed.filters,
            excluded: &[],
        };
        assert_eq!(
            matching_shows_in(&conn, &parsed.text, scope, 10).unwrap(),
            ["TV Show"]
        );
    }
}
