//! Media rows, subtitle cues, dialogue search, and the library/Wanted/browse queries.

use super::*;
use crate::querysyntax::{Field, Filter};

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

/// A dialogue-search hit: a cue plus the media it belongs to.
#[derive(Debug, Clone, Serialize)]
pub struct SearchHit {
    pub media_id: i64,
    pub rating_key: String,
    pub kind: String,
    pub title: String,
    pub show: Option<String>,
    pub season: Option<i64>,
    pub episode: Option<i64>,
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
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

pub fn insert_cues(
    conn: &mut rusqlite::Connection,
    media_id: i64,
    cues: &[crate::subtitles::Cue],
) -> Result<usize> {
    let tx = conn.transaction()?;
    {
        let mut stmt = tx.prepare(
            "INSERT INTO subtitle_cue (media_id, start_ms, end_ms, text) VALUES (?1,?2,?3,?4)",
        )?;
        for cue in cues {
            stmt.execute(params![media_id, cue.start_ms, cue.end_ms, cue.text])?;
        }
    }
    tx.execute(
        "UPDATE media SET cue_count = cue_count + ?1 WHERE id = ?2",
        params![cues.len() as i64, media_id],
    )?;
    tx.commit()?;
    Ok(cues.len())
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

pub fn cues_in_window(
    conn: &rusqlite::Connection,
    media_id: i64,
    start_ms: i64,
    end_ms: i64,
) -> Result<Vec<crate::subtitles::Cue>> {
    let mut stmt = conn.prepare(
        "SELECT start_ms, end_ms, text FROM subtitle_cue
         WHERE media_id = ?1 AND end_ms > ?2 AND start_ms < ?3
         ORDER BY start_ms",
    )?;
    let rows = stmt.query_map(params![media_id, start_ms, end_ms], |r| {
        Ok(crate::subtitles::Cue {
            start_ms: r.get(0)?,
            end_ms: r.get(1)?,
            text: r.get(2)?,
        })
    })?;
    Ok(rows.filter_map(Result::ok).collect())
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

/// `AND <col> IN (?n, ...)` for the selected libraries, and the values to bind
/// to those placeholders. No libraries selected means all of them: an empty
/// clause. `first_param` is the number of the first placeholder to use.
pub(super) fn library_clause(
    col: &str,
    libraries: &[String],
    first_param: usize,
) -> (String, Vec<rusqlite::types::Value>) {
    if libraries.is_empty() {
        return (String::new(), Vec::new());
    }
    let placeholders: Vec<String> = (0..libraries.len())
        .map(|i| format!("?{}", first_param + i))
        .collect();
    (
        format!(" AND {col} IN ({})", placeholders.join(",")),
        libraries
            .iter()
            .cloned()
            .map(rusqlite::types::Value::from)
            .collect(),
    )
}

/// What a search is limited to, beyond its text: libraries (the picker), the
/// operators typed into the box (`show:`, `-ep:` ... see `querysyntax`) and the
/// words to leave out (`-word`).
#[derive(Debug, Clone, Copy, Default)]
pub struct Scope<'a> {
    pub libraries: &'a [String],
    pub filters: &'a [Filter],
    pub excluded: &'a [String],
}

impl<'a> Scope<'a> {
    pub fn libraries(libraries: &'a [String]) -> Self {
        Scope {
            libraries,
            ..Scope::default()
        }
    }
}

fn like_pattern(value: &str) -> String {
    format!("%{}%", value.replace(['%', '_'], ""))
}

/// `AND (...)` clauses for the search operators, on the `media` row aliased `m`,
/// with the values to bind. `first_param` is the number of the first placeholder.
/// `show:` and the like never exclude a movie by accident: `-show:x` keeps rows
/// that have no show at all.
pub(super) fn filter_clause(
    filters: &[Filter],
    first_param: usize,
) -> (String, Vec<rusqlite::types::Value>) {
    let mut sql = String::new();
    let mut binds: Vec<rusqlite::types::Value> = Vec::new();
    for f in filters {
        let n = first_param + binds.len();
        let cond = match (f.field, f.value.is_empty()) {
            (Field::Show, _) => {
                binds.push(like_pattern(&f.value).into());
                if f.negate {
                    format!("(m.show IS NULL OR m.show NOT LIKE ?{n})")
                } else {
                    format!("m.show LIKE ?{n}")
                }
            }
            (Field::Library, _) => {
                binds.push(f.value.clone().into());
                if f.negate {
                    format!("(m.library IS NULL OR m.library <> ?{n} COLLATE NOCASE)")
                } else {
                    format!("m.library = ?{n} COLLATE NOCASE")
                }
            }
            (Field::Season, _) => {
                binds.push(f.value.parse::<i64>().unwrap_or(-1).into());
                if f.negate {
                    format!("(m.season IS NULL OR m.season <> ?{n})")
                } else {
                    format!("m.season = ?{n}")
                }
            }
            (field @ (Field::Episode | Field::Movie), empty) => {
                let kind = if field == Field::Episode {
                    "episode"
                } else {
                    "movie"
                };
                let body = if empty {
                    format!("m.kind = '{kind}'")
                } else {
                    binds.push(like_pattern(&f.value).into());
                    format!("(m.kind = '{kind}' AND m.title LIKE ?{n})")
                };
                if f.negate {
                    format!("NOT {body}")
                } else {
                    body
                }
            }
        };
        sql.push_str(" AND ");
        sql.push_str(&cond);
    }
    (sql, binds)
}

/// The library picker and the operators together, as one `AND ...` string.
fn scope_clause(scope: &Scope, first_param: usize) -> (String, Vec<rusqlite::types::Value>) {
    let (mut sql, mut binds) = library_clause("m.library", scope.libraries, first_param);
    let (fsql, fbinds) = filter_clause(scope.filters, first_param + binds.len());
    sql.push_str(&fsql);
    binds.extend(fbinds);
    (sql, binds)
}

/// Is there anything in `text` the full-text index could look for?
pub fn has_search_text(text: &str) -> bool {
    text.chars().any(char::is_alphanumeric)
}

/// An FTS5 query: the text as a phrase, minus any excluded words
/// (`"a b" NOT "c" NOT "d"`). Empty when there is no text to look for, since
/// FTS5 can't match on exclusions alone.
pub(super) fn fts_query(text: &str, excluded: &[String]) -> String {
    let mut q = to_fts_phrase(text);
    if q.is_empty() {
        return q;
    }
    for word in excluded {
        let w = to_fts_phrase(word);
        if !w.is_empty() {
            q.push_str(" NOT ");
            q.push_str(&w);
        }
    }
    q
}

/// Full-text dialogue search across every library. `query` is a user phrase;
/// we quote it as an FTS5 phrase so punctuation doesn't trip the query parser.
pub fn search_dialogue(
    conn: &rusqlite::Connection,
    query: &str,
    limit: i64,
) -> Result<Vec<SearchHit>> {
    Ok(search_dialogue_page(conn, query, Scope::default(), limit, 0)?.0)
}

/// One page of dialogue search, best match first, optionally limited to some
/// libraries (none = all). The bool says whether more matches follow this page
/// (found by asking for one extra row, so no slow count over the whole index).
pub fn search_dialogue_page(
    conn: &rusqlite::Connection,
    query: &str,
    scope: Scope,
    limit: i64,
    offset: i64,
) -> Result<(Vec<SearchHit>, bool)> {
    let fts_query = fts_query(query, scope.excluded);
    if fts_query.is_empty() {
        return Ok((Vec::new(), false));
    }
    let (lib_sql, lib_vals) = scope_clause(&scope, 4);
    let mut stmt = conn.prepare(&format!(
        "SELECT m.id, m.rating_key, m.kind, m.title, m.show, m.season, m.episode,
                c.start_ms, c.end_ms, c.text
         FROM subtitle_fts f
         JOIN subtitle_cue c ON c.id = f.rowid
         JOIN media m ON m.id = c.media_id
         WHERE subtitle_fts MATCH ?1{lib_sql}
         ORDER BY bm25(subtitle_fts)
         LIMIT ?2 OFFSET ?3"
    ))?;
    let mut binds: Vec<rusqlite::types::Value> =
        vec![fts_query.into(), (limit + 1).into(), offset.into()];
    binds.extend(lib_vals);
    let rows = stmt.query_map(rusqlite::params_from_iter(binds), |r| {
        Ok(SearchHit {
            media_id: r.get(0)?,
            rating_key: r.get(1)?,
            kind: r.get(2)?,
            title: r.get(3)?,
            show: r.get(4)?,
            season: r.get(5)?,
            episode: r.get(6)?,
            start_ms: r.get(7)?,
            end_ms: r.get(8)?,
            text: r.get(9)?,
        })
    })?;
    let mut hits: Vec<SearchHit> = rows.filter_map(Result::ok).collect();
    let has_more = hits.len() as i64 > limit;
    hits.truncate(limit.max(0) as usize);
    Ok((hits, has_more))
}

/// An item found by title (movie or episode).
#[derive(Debug, Clone, Serialize)]
pub struct TitleHit {
    pub rating_key: String,
    pub kind: String,
    pub title: String,
    pub show: Option<String>,
    pub season: Option<i64>,
    pub episode: Option<i64>,
    pub year: Option<i64>,
    pub duration_ms: i64,
    /// Whether any subtitle lines are indexed for it. `false` is what lets the
    /// UI offer "no subtitles -> find some" right in the results.
    pub has_subtitles: bool,
}

/// One page of title search (matches the item title or its show's name),
/// optionally limited to some libraries (none = all). The bool says whether
/// more matches follow this page.
pub fn search_titles(
    conn: &rusqlite::Connection,
    query: &str,
    scope: Scope,
    limit: i64,
    offset: i64,
) -> Result<(Vec<TitleHit>, bool)> {
    let like = like_pattern(query);
    let (mut lib_sql, lib_vals) = scope_clause(&scope, 4);
    let mut binds: Vec<rusqlite::types::Value> =
        vec![like.into(), (limit + 1).into(), offset.into()];
    binds.extend(lib_vals);
    // `-word`: leave out titles (or shows) containing it.
    for word in scope.excluded {
        let n = binds.len() + 1;
        lib_sql.push_str(&format!(
            " AND NOT (m.title LIKE ?{n} OR COALESCE(m.show, '') LIKE ?{n})"
        ));
        binds.push(like_pattern(word).into());
    }
    let mut stmt = conn.prepare(&format!(
        "SELECT m.rating_key, m.kind, m.title, m.show, m.season, m.episode, m.year, m.duration_ms,
                EXISTS (SELECT 1 FROM subtitle_cue c WHERE c.media_id = m.id)
         FROM media m
         WHERE (m.title LIKE ?1 OR m.show LIKE ?1){lib_sql}
         ORDER BY m.show, m.season, m.episode, m.title
         LIMIT ?2 OFFSET ?3"
    ))?;
    let rows = stmt.query_map(rusqlite::params_from_iter(binds), |r| {
        Ok(TitleHit {
            rating_key: r.get(0)?,
            kind: r.get(1)?,
            title: r.get(2)?,
            show: r.get(3)?,
            season: r.get(4)?,
            episode: r.get(5)?,
            year: r.get(6)?,
            duration_ms: r.get(7)?,
            has_subtitles: r.get::<_, i64>(8)? != 0,
        })
    })?;
    let mut hits: Vec<TitleHit> = rows.filter_map(Result::ok).collect();
    let has_more = hits.len() as i64 > limit;
    hits.truncate(limit.max(0) as usize);
    Ok((hits, has_more))
}

/// Turn a free-text query into a safe FTS5 phrase match: keep alphanumerics and
/// spaces, wrap the remaining tokens in double quotes.
pub(super) fn to_fts_phrase(query: &str) -> String {
    let cleaned: String = query
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect();
    let tokens: Vec<&str> = cleaned.split_whitespace().collect();
    if tokens.is_empty() {
        String::new()
    } else {
        format!("\"{}\"", tokens.join(" "))
    }
}

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
    // title — so searching an episode title (e.g. "Ozymandias") still
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
/// the regular results, so searching "Breaking Bad" finds the show even if
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
    fn fts_phrase_is_sanitized() {
        assert_eq!(
            to_fts_phrase("I am the one who knocks!"),
            "\"I am the one who knocks\""
        );
        assert_eq!(to_fts_phrase("  '''  "), "");
    }

    #[test]
    fn insert_and_search_roundtrip() {
        let pool = mem_pool();
        let mut conn = pool.get().unwrap();
        let id = upsert_media(
            &conn,
            "123",
            "episode",
            "Pilot",
            Some("Breaking Bad"),
            Some(1),
            Some(1),
            Some(2008),
            "/media/bb/s01e01.mkv",
            None,
            2_820_000,
            "direct",
            Some("TV Shows"),
            111,
            222,
        )
        .unwrap();
        let cues = vec![
            Cue {
                start_ms: 1000,
                end_ms: 2500,
                text: "I am the one who knocks".into(),
            },
            Cue {
                start_ms: 3000,
                end_ms: 4000,
                text: "Say my name".into(),
            },
        ];
        insert_cues(&mut conn, id, &cues).unwrap();

        let hits = search_dialogue(&conn, "one who knocks", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].text, "I am the one who knocks");
        assert_eq!(hits[0].show.as_deref(), Some("Breaking Bad"));

        let s = stats(&conn).unwrap();
        assert_eq!(s.media_count, 1);
        assert_eq!(s.cue_count, 2);
    }

    #[test]
    fn library_stats_breaks_down_by_reason() {
        let pool = mem_pool();
        let mut conn = pool.get().unwrap();
        // indexed (direct + cues)
        let a = upsert_media(
            &conn,
            "a",
            "movie",
            "A",
            None,
            None,
            None,
            None,
            "/m/a.mkv",
            None,
            0,
            "direct",
            Some("Movies"),
            1,
            1,
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
            "b",
            "movie",
            "B",
            None,
            None,
            None,
            None,
            "/m/b.mkv",
            None,
            0,
            "direct",
            Some("Movies"),
            1,
            1,
        )
        .unwrap();
        // unreachable (download mode)
        upsert_media(
            &conn,
            "c",
            "movie",
            "C",
            None,
            None,
            None,
            None,
            "D:\\x.mkv",
            None,
            0,
            "plex-download",
            Some("Movies"),
            0,
            0,
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
            "m1",
            "movie",
            "Dune",
            None,
            None,
            None,
            Some(2021),
            "/m/dune.mkv",
            None,
            0,
            "direct",
            Some("Movies"),
            1,
            1,
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
            "e1",
            "episode",
            "Pilot",
            Some("BB"),
            Some(1),
            Some(1),
            Some(2008),
            "/m/bb1.mkv",
            None,
            0,
            "direct",
            Some("TV"),
            1,
            1,
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
            "e2",
            "episode",
            "Cat's in the Bag",
            Some("BB"),
            Some(1),
            Some(2),
            Some(2008),
            "/m/bb2.mkv",
            None,
            0,
            "direct",
            Some("TV"),
            1,
            1,
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
            "m1",
            "movie",
            "Dune",
            None,
            None,
            None,
            Some(2021),
            "/m/dune.mkv",
            None,
            0,
            "direct",
            Some("Movies"),
            1,
            1,
        )
        .unwrap();
        upsert_media(
            &conn,
            "e1",
            "episode",
            "Pilot",
            Some("Breaking Bad"),
            Some(1),
            Some(1),
            Some(2008),
            "/m/bb1.mkv",
            None,
            0,
            "direct",
            Some("TV"),
            1,
            1,
        )
        .unwrap();
        upsert_media(
            &conn,
            "e2",
            "episode",
            "Ozymandias",
            Some("Breaking Bad"),
            Some(5),
            Some(14),
            Some(2013),
            "/m/bb2.mkv",
            None,
            0,
            "direct",
            Some("TV"),
            1,
            1,
        )
        .unwrap();

        // Movie title matches directly.
        let (entries, total) =
            browse_library(&conn, "Dune", &[], BrowseStatus::All, 50, 0).unwrap();
        assert_eq!(total, 1);
        assert_eq!(entries[0].kind, "movie");

        // Show name matches directly.
        let (entries, total) =
            browse_library(&conn, "Breaking Bad", &[], BrowseStatus::All, 50, 0).unwrap();
        assert_eq!(total, 1);
        assert_eq!(entries[0].kind, "show");

        // An individual EPISODE's own title must also surface its show — this
        // is the gap that was fixed: searching "Ozymandias" previously found
        // nothing, because only the show name was matched for episode rows.
        let (entries, total) =
            browse_library(&conn, "Ozymandias", &[], BrowseStatus::All, 50, 0).unwrap();
        assert_eq!(total, 1, "episode title search should find the parent show");
        assert_eq!(entries[0].kind, "show");
        assert_eq!(entries[0].name, "Breaking Bad");

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
            "m1",
            "movie",
            "Dune",
            None,
            None,
            None,
            Some(2021),
            "/m/dune.mkv",
            None,
            0,
            "direct",
            Some("Movies"),
            1,
            1,
        )
        .unwrap();
        upsert_media(
            &conn,
            "e1",
            "episode",
            "Pilot",
            Some("Breaking Bad"),
            Some(1),
            Some(1),
            Some(2008),
            "/m/bb1.mkv",
            None,
            0,
            "direct",
            Some("TV"),
            1,
            1,
        )
        .unwrap();

        // No library filter: both the movie and the show are visible.
        let (entries, total) = browse_library(&conn, "", &[], BrowseStatus::All, 50, 0).unwrap();
        assert_eq!(total, 2);
        assert_eq!(entries.len(), 2);

        // Scoped to "Movies": only Dune.
        let (entries, total) =
            browse_library(&conn, "", &["Movies".to_string()], BrowseStatus::All, 50, 0).unwrap();
        assert_eq!(total, 1);
        assert_eq!(entries[0].name, "Dune");

        // Scoped to "TV": only Breaking Bad.
        let (entries, total) =
            browse_library(&conn, "", &["TV".to_string()], BrowseStatus::All, 50, 0).unwrap();
        assert_eq!(total, 1);
        assert_eq!(entries[0].name, "Breaking Bad");

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
            "m1",
            "movie",
            "Breaking Point",
            None,
            None,
            None,
            Some(2019),
            "/m/bp.mkv",
            None,
            0,
            "direct",
            Some("Movies"),
            1,
            1,
        )
        .unwrap();
        upsert_media(
            &conn,
            "e1",
            "episode",
            "Pilot",
            Some("Breaking Bad"),
            Some(1),
            Some(1),
            Some(2008),
            "/m/bb1.mkv",
            None,
            0,
            "direct",
            Some("TV"),
            1,
            1,
        )
        .unwrap();
        upsert_media(
            &conn,
            "e2",
            "episode",
            "Ozymandias",
            Some("Breaking Bad"),
            Some(5),
            Some(14),
            Some(2013),
            "/m/bb2.mkv",
            None,
            0,
            "direct",
            Some("TV"),
            1,
            1,
        )
        .unwrap();

        // Matches the show, deduplicated across its two episodes -- and not
        // the movie, even though its title also contains "Breaking".
        let shows = matching_shows(&conn, "Breaking", 50).unwrap();
        assert_eq!(shows, vec!["Breaking Bad".to_string()]);

        // An episode's own title doesn't match a show search (that's
        // `browse_library`'s job, which deliberately also checks episode
        // titles) -- this function is specifically "does the query look
        // like a show name."
        assert!(matching_shows(&conn, "Ozymandias", 50).unwrap().is_empty());

        assert!(matching_shows(&conn, "nonexistent", 50).unwrap().is_empty());
        assert!(matching_shows(&conn, "", 50).unwrap().is_empty());
        assert!(matching_shows(&conn, "   ", 50).unwrap().is_empty());
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
    fn prune_unmapped_libraries_removes_only_unmapped_tagged_rows() {
        let pool = mem_pool();
        let conn = pool.get().unwrap();
        upsert_media(
            &conn,
            "a",
            "movie",
            "A",
            None,
            None,
            None,
            None,
            "/a.mkv",
            None,
            0,
            "direct",
            Some("Movies"),
            1,
            1,
        )
        .unwrap();
        upsert_media(
            &conn,
            "b",
            "movie",
            "B",
            None,
            None,
            None,
            None,
            "/b.mkv",
            None,
            0,
            "direct",
            Some("Anime"),
            1,
            1,
        )
        .unwrap();
        upsert_media(
            &conn, "c", "movie", "C", None, None, None, None, "/c.mkv", None, 0, "direct", None, 1,
            1,
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

    /// Three libraries, each with `n` episodes that all say "knock knock".
    fn seed_libraries(conn: &mut rusqlite::Connection, per_library: usize) {
        for lib in ["TV", "Anime", "Movies"] {
            for i in 0..per_library {
                let key = format!("{lib}-{i}");
                let kind = if lib == "Movies" { "movie" } else { "episode" };
                let show = (lib != "Movies").then(|| format!("{lib} Show"));
                let id = add_media(
                    conn,
                    &key,
                    kind,
                    &format!("{lib} item {i}"),
                    show.as_deref(),
                    "direct",
                    lib,
                );
                insert_cues(
                    conn,
                    id,
                    &[Cue {
                        start_ms: 1000,
                        end_ms: 2000,
                        text: "knock knock who is there".into(),
                    }],
                )
                .unwrap();
            }
        }
    }

    fn cue(i: i64) -> Cue {
        Cue {
            start_ms: i * 1000,
            end_ms: i * 1000 + 500,
            text: format!("line {i}"),
        }
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

    fn names(libs: &[&str]) -> Vec<String> {
        libs.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn library_clause_is_empty_for_all_and_numbers_its_placeholders() {
        assert_eq!(library_clause("m.library", &[], 4).0, "");
        let (sql, vals) = library_clause("m.library", &names(&["TV", "Anime"]), 4);
        assert_eq!(sql, " AND m.library IN (?4,?5)");
        assert_eq!(vals.len(), 2);
    }

    #[test]
    fn dialogue_search_can_be_limited_to_some_libraries() {
        let pool = mem_pool();
        let mut conn = pool.get().unwrap();
        seed_libraries(&mut conn, 3);
        let libs_of = |libs: &[&str]| -> Vec<String> {
            let (hits, _) =
                search_dialogue_page(&conn, "knock knock", Scope::libraries(&names(libs)), 50, 0)
                    .unwrap();
            let mut v: Vec<String> = hits
                .iter()
                .map(|h| h.rating_key.split('-').next().unwrap().to_string())
                .collect();
            v.sort();
            v.dedup();
            v
        };
        assert_eq!(
            libs_of(&[]),
            ["Anime", "Movies", "TV"],
            "none selected = all"
        );
        assert_eq!(libs_of(&["TV"]), ["TV"]);
        assert_eq!(libs_of(&["TV", "Movies"]), ["Movies", "TV"]);
        assert!(libs_of(&["Nope"]).is_empty());
    }

    #[test]
    fn dialogue_search_pages_and_says_whether_more_follow() {
        let pool = mem_pool();
        let mut conn = pool.get().unwrap();
        seed_libraries(&mut conn, 4); // 12 matches in all
        let (first, more) =
            search_dialogue_page(&conn, "knock knock", Scope::default(), 5, 0).unwrap();
        assert_eq!((first.len(), more), (5, true));
        let (second, more) =
            search_dialogue_page(&conn, "knock knock", Scope::default(), 5, 5).unwrap();
        assert_eq!((second.len(), more), (5, true));
        let (last, more) =
            search_dialogue_page(&conn, "knock knock", Scope::default(), 5, 10).unwrap();
        assert_eq!((last.len(), more), (2, false));
        // pages don't overlap and together are every match
        let mut keys: Vec<String> = first
            .iter()
            .chain(&second)
            .chain(&last)
            .map(|h| h.rating_key.clone())
            .collect();
        keys.sort();
        keys.dedup();
        assert_eq!(keys.len(), 12);
        // exactly a full page, nothing after it: no "more"
        let (all, more) =
            search_dialogue_page(&conn, "knock knock", Scope::default(), 12, 0).unwrap();
        assert_eq!((all.len(), more), (12, false));
    }

    #[test]
    fn title_search_filters_by_library_and_pages() {
        let pool = mem_pool();
        let mut conn = pool.get().unwrap();
        seed_libraries(&mut conn, 3);
        let (all, _) = search_titles(&conn, "item", Scope::default(), 50, 0).unwrap();
        assert_eq!(all.len(), 9);
        let (tv, _) =
            search_titles(&conn, "item", Scope::libraries(&names(&["TV"])), 50, 0).unwrap();
        assert_eq!(tv.len(), 3);
        assert!(tv.iter().all(|h| h.rating_key.starts_with("TV-")));
        let (two, _) = search_titles(
            &conn,
            "item",
            Scope::libraries(&names(&["TV", "Movies"])),
            50,
            0,
        )
        .unwrap();
        assert_eq!(two.len(), 6);
        let (page, more) = search_titles(&conn, "item", Scope::default(), 4, 8).unwrap();
        assert_eq!((page.len(), more), (1, false));
        let (page, more) = search_titles(&conn, "item", Scope::default(), 4, 0).unwrap();
        assert_eq!((page.len(), more), (4, true));
    }

    #[test]
    fn title_hits_say_whether_they_have_subtitles() {
        let pool = mem_pool();
        let mut conn = pool.get().unwrap();
        seed_libraries(&mut conn, 1);
        // an item with no cues at all
        add_media(
            &conn,
            "bare",
            "movie",
            "Item Bare",
            None,
            "direct",
            "Movies",
        );
        let (hits, _) = search_titles(&conn, "item", Scope::default(), 50, 0).unwrap();
        assert_eq!(hits.len(), 4);
        for h in &hits {
            assert_eq!(h.has_subtitles, h.rating_key != "bare", "{}", h.rating_key);
        }
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

    /// Every dialogue match for a query typed the way a person would (operators
    /// and all), as sorted rating keys.
    fn typed(conn: &rusqlite::Connection, q: &str) -> Vec<String> {
        let parsed = crate::querysyntax::parse(q);
        let scope = Scope {
            libraries: &[],
            filters: &parsed.filters,
            excluded: &parsed.excluded,
        };
        let (hits, _) = search_dialogue_page(conn, &parsed.text, scope, 500, 0).unwrap();
        let mut keys: Vec<String> = hits.into_iter().map(|h| h.rating_key).collect();
        keys.sort();
        keys
    }

    fn typed_titles(conn: &rusqlite::Connection, q: &str) -> Vec<String> {
        let parsed = crate::querysyntax::parse(q);
        let scope = Scope {
            libraries: &[],
            filters: &parsed.filters,
            excluded: &parsed.excluded,
        };
        let (hits, _) = search_titles(conn, &parsed.text, scope, 500, 0).unwrap();
        let mut keys: Vec<String> = hits.into_iter().map(|h| h.rating_key).collect();
        keys.sort();
        keys
    }

    #[test]
    fn fts_queries_carry_the_excluded_words() {
        assert_eq!(fts_query("knock knock", &[]), "\"knock knock\"");
        assert_eq!(
            fts_query("knock", &names(&["walter", "say my name"])),
            "\"knock\" NOT \"walter\" NOT \"say my name\""
        );
        // nothing to look for: exclusions alone can't be searched
        assert_eq!(fts_query("  ", &names(&["walter"])), "");
        // an exclusion that is only punctuation is dropped
        assert_eq!(fts_query("knock", &names(&["!!!"])), "\"knock\"");
        assert!(has_search_text("a") && !has_search_text(" - !"));
    }

    #[test]
    fn show_operator_limits_dialogue_to_that_show() {
        let pool = mem_pool();
        let mut conn = pool.get().unwrap();
        seed_libraries(&mut conn, 2); // TV-0,1 / Anime-0,1 / Movies-0,1
        assert_eq!(typed(&conn, "knock show:\"TV Show\""), ["TV-0", "TV-1"]);
        assert_eq!(typed(&conn, "knock show:anime"), ["Anime-0", "Anime-1"]);
        assert_eq!(
            typed(&conn, "show:show knock").len(),
            4,
            "contains, any case: both shows"
        );
    }

    #[test]
    fn excluding_a_show_keeps_movies_that_have_no_show() {
        let pool = mem_pool();
        let mut conn = pool.get().unwrap();
        seed_libraries(&mut conn, 2);
        assert_eq!(
            typed(&conn, "knock -show:\"TV Show\""),
            ["Anime-0", "Anime-1", "Movies-0", "Movies-1"]
        );
    }

    #[test]
    fn movie_and_ep_operators_pick_the_kind_and_match_the_title() {
        let pool = mem_pool();
        let mut conn = pool.get().unwrap();
        seed_libraries(&mut conn, 2);
        assert_eq!(typed(&conn, "knock movie:"), ["Movies-0", "Movies-1"]);
        assert_eq!(typed(&conn, "knock movie:\"item 1\""), ["Movies-1"]);
        assert_eq!(typed(&conn, "knock ep:").len(), 4, "episodes only");
        assert_eq!(typed(&conn, "knock ep:\"tv item 0\""), ["TV-0"]);
        // ep: never matches a movie even if the title would
        assert!(typed(&conn, "knock ep:movies").is_empty());
        assert_eq!(
            typed(&conn, "knock -movie:"),
            ["Anime-0", "Anime-1", "TV-0", "TV-1"]
        );
        assert_eq!(
            typed(&conn, "knock -ep:\"item 0\""),
            ["Anime-1", "Movies-0", "Movies-1", "TV-1"]
        );
    }

    #[test]
    fn library_and_season_operators() {
        let pool = mem_pool();
        let mut conn = pool.get().unwrap();
        seed_libraries(&mut conn, 2);
        assert_eq!(typed(&conn, "knock library:tv"), ["TV-0", "TV-1"]);
        assert_eq!(typed(&conn, "knock -lib:TV").len(), 4);
        assert_eq!(
            typed(&conn, "knock season:1").len(),
            4,
            "episodes are season 1; movies have none"
        );
        assert!(typed(&conn, "knock season:2").is_empty());
        assert_eq!(typed(&conn, "knock -season:1"), ["Movies-0", "Movies-1"]);
    }

    #[test]
    fn operators_combine_with_each_other_and_the_picker() {
        let pool = mem_pool();
        let mut conn = pool.get().unwrap();
        seed_libraries(&mut conn, 3);
        assert_eq!(
            typed(&conn, "knock show:show ep:\"item 2\" -show:anime"),
            ["TV-2"]
        );
        let parsed = crate::querysyntax::parse("knock -movie:");
        let libs = names(&["Anime", "Movies"]);
        let scope = Scope {
            libraries: &libs,
            filters: &parsed.filters,
            excluded: &[],
        };
        let (hits, _) = search_dialogue_page(&conn, "knock", scope, 50, 0).unwrap();
        assert_eq!(
            hits.len(),
            3,
            "picker (Anime, Movies) AND no movies = Anime only"
        );
        assert!(hits.iter().all(|h| h.rating_key.starts_with("Anime-")));
    }

    #[test]
    fn excluded_words_drop_lines_that_contain_them() {
        let pool = mem_pool();
        let mut conn = pool.get().unwrap();
        // the seeded line is "knock knock who is there": add one with another word
        seed_libraries(&mut conn, 1);
        let id = add_media(&conn, "extra", "movie", "Extra", None, "direct", "Movies");
        insert_cues(
            &mut conn,
            id,
            &[Cue {
                start_ms: 0,
                end_ms: 1,
                text: "knock knock walter".into(),
            }],
        )
        .unwrap();
        assert_eq!(typed(&conn, "knock").len(), 4);
        assert_eq!(typed(&conn, "knock -walter").len(), 3);
        assert_eq!(
            typed(&conn, "knock -\"who is\"").len(),
            1,
            "a quoted phrase is excluded as a phrase"
        );
    }

    #[test]
    fn title_search_takes_operators_and_exclusions_too() {
        let pool = mem_pool();
        let mut conn = pool.get().unwrap();
        seed_libraries(&mut conn, 3); // titles "TV item 0" ... "Movies item 2"
        assert_eq!(typed_titles(&conn, "show:anime").len(), 3);
        assert_eq!(
            typed_titles(&conn, "item 1 -show:anime").len(),
            2,
            "TV-1 and Movies-1"
        );
        assert_eq!(typed_titles(&conn, "item movie:").len(), 3);
        assert_eq!(
            typed_titles(&conn, "item -tv").len(),
            6,
            "-word leaves out titles (or shows) containing it"
        );
        assert_eq!(
            typed_titles(&conn, "ep:\"item 2\"").len(),
            2,
            "TV-2 and Anime-2, not the movie"
        );
        assert_eq!(
            typed_titles(&conn, "").len(),
            9,
            "no text and no operators: everything"
        );
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
