//! Searching dialogue (FTS) and titles, with the library scope and the search operators.

use super::*;
use crate::querysyntax::{Field, Filter};

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

pub(super) fn like_pattern(value: &str) -> String {
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
pub(super) fn scope_clause(
    scope: &Scope,
    first_param: usize,
) -> (String, Vec<rusqlite::types::Value>) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::subtitles::Cue;

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
    fn fts_phrase_is_sanitized() {
        assert_eq!(
            to_fts_phrase("I am the one who whistles!"),
            "\"I am the one who whistles\""
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
            Some("The Lantern Cove"),
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
                text: "I am the one who whistles".into(),
            },
            Cue {
                start_ms: 3000,
                end_ms: 4000,
                text: "Say it with pie".into(),
            },
        ];
        insert_cues(&mut conn, id, &cues).unwrap();

        let hits = search_dialogue(&conn, "one who whistles", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].text, "I am the one who whistles");
        assert_eq!(hits[0].show.as_deref(), Some("The Lantern Cove"));

        let s = stats(&conn).unwrap();
        assert_eq!(s.media_count, 1);
        assert_eq!(s.cue_count, 2);
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
    fn fts_queries_carry_the_excluded_words() {
        assert_eq!(fts_query("knock knock", &[]), "\"knock knock\"");
        assert_eq!(
            fts_query("knock", &names(&["dale", "say it with pie"])),
            "\"knock\" NOT \"dale\" NOT \"say it with pie\""
        );
        // nothing to look for: exclusions alone can't be searched
        assert_eq!(fts_query("  ", &names(&["dale"])), "");
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
                text: "knock knock dale".into(),
            }],
        )
        .unwrap();
        assert_eq!(typed(&conn, "knock").len(), 4);
        assert_eq!(typed(&conn, "knock -dale").len(), 3);
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
}
