//! Bazarr (the subtitle manager that sits beside Sonarr and Radarr).
//!
//! What we ask of it: find the movie or episode that a file in our library is,
//! tell it to search for the subtitles it is missing, ask it to re-time (sync)
//! a subtitle it wrote, and learn which languages it is set up to keep, so the
//! indexer can prefer them. Bazarr writes its subtitles beside the video, which
//! the indexer already reads; this only makes it work with them.
//!
//! The API used (see Bazarr's `bazarr/api`): `X-API-KEY` header; `GET
//! system/status`, `system/languages/profiles`, `movies`, `series`,
//! `episodes?seriesid[]=`; `PATCH movies?action=search-missing`,
//! `episodes/subtitles`, `subtitles?action=sync`.

use anyhow::{bail, Context, Result};
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// How long a list fetched from Bazarr (all movies, all series) is reused.
const CACHE_TTL: Duration = Duration::from_secs(60);

/// A language Bazarr is missing a subtitle in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Need {
    pub code2: String,
    pub forced: bool,
    pub hi: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Movie,
    Episode,
}

/// The movie or episode in Bazarr that a library file is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Target {
    pub kind: Kind,
    /// Radarr id (movie) or Sonarr episode id (episode).
    pub id: i64,
    pub series_id: Option<i64>,
    pub title: String,
    /// The video's path as Bazarr sees it.
    pub path: String,
    pub missing: Vec<Need>,
    /// The languages of the profile this item uses, in order; empty if unknown.
    #[serde(skip)]
    pub profile_id: Option<i64>,
}

// ---- matching paths ---------------------------------------------------------

fn components(path: &str) -> Vec<String> {
    path.replace('\\', "/")
        .to_lowercase()
        .split('/')
        .filter(|c| !c.is_empty())
        .map(str::to_string)
        .collect()
}

/// How many trailing path components `a` and `b` share (0 if even the file
/// names differ). The two apps mount the media at different places, so the
/// front of the path can't be compared; the end of it can.
pub fn tail_score(a: &str, b: &str) -> usize {
    let (a, b) = (components(a), components(b));
    a.iter()
        .rev()
        .zip(b.iter().rev())
        .take_while(|(x, y)| x == y)
        .count()
}

/// The single entry with the best (non-zero) score. `None` if nothing matches
/// or if two different entries tie for best, so we never act on a guess.
pub fn best_match<T>(entries: &[T], score: impl Fn(&T) -> usize) -> Option<&T> {
    let mut best: Option<(&T, usize)> = None;
    let mut tied = false;
    for e in entries {
        let s = score(e);
        if s == 0 {
            continue;
        }
        match best {
            Some((_, b)) if s < b => {}
            Some((_, b)) if s == b => tied = true,
            _ => {
                best = Some((e, s));
                tied = false;
            }
        }
    }
    if tied {
        None
    } else {
        best.map(|(e, _)| e)
    }
}

/// How well a series folder (`series_path`, as Bazarr knows it) matches a
/// file: the folder's name has to be one of the file's folders; more of the
/// folder's own path matching before that counts for more.
pub fn series_score(series_path: &str, file_path: &str) -> usize {
    let series = components(series_path);
    let file = components(file_path);
    let (Some(last), true) = (series.last(), file.len() > 1) else {
        return 0;
    };
    // The file's folders only (the last component is the file name).
    let dirs = &file[..file.len() - 1];
    let mut best = 0;
    for (i, d) in dirs.iter().enumerate() {
        if d != last {
            continue;
        }
        let run = series
            .iter()
            .rev()
            .zip(dirs[..=i].iter().rev())
            .take_while(|(x, y)| x == y)
            .count();
        best = best.max(run);
    }
    best
}

// ---- reading Bazarr's answers -----------------------------------------------

/// Bazarr wraps lists in `{"data": [...]}` on most endpoints and returns a bare
/// list on a few; accept both.
fn items(v: &Value) -> &[Value] {
    v.get("data")
        .and_then(Value::as_array)
        .or_else(|| v.as_array())
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

/// Bazarr stores some flags as the strings "True"/"False".
fn truthy(v: Option<&Value>) -> bool {
    match v {
        Some(Value::Bool(b)) => *b,
        Some(Value::String(s)) => s.eq_ignore_ascii_case("true"),
        _ => false,
    }
}

fn needs(v: &Value) -> Vec<Need> {
    v.get("missing_subtitles")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|m| {
                    let code = m.get("code2").and_then(Value::as_str)?;
                    Some(Need {
                        code2: code.to_lowercase(),
                        forced: truthy(m.get("forced")),
                        hi: truthy(m.get("hi")),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn text(v: &Value, key: &str) -> String {
    v.get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn int(v: &Value, key: &str) -> Option<i64> {
    v.get(key).and_then(Value::as_i64)
}

/// The languages in Bazarr's profiles, first profile first, each once, as
/// 2-letter codes.
pub fn parse_languages(profiles: &Value) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for profile in items(profiles) {
        for item in profile
            .get("items")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or(&[])
        {
            let Some(code) = item
                .get("language")
                .and_then(Value::as_str)
                .and_then(crate::langs::code2)
            else {
                continue;
            };
            if !out.iter().any(|c| c == code) {
                out.push(code.to_string());
            }
        }
    }
    out
}

// ---- the client ---------------------------------------------------------------

fn cache() -> &'static Mutex<HashMap<String, (Instant, Value)>> {
    static CACHE: OnceLock<Mutex<HashMap<String, (Instant, Value)>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

#[derive(Clone)]
pub struct Client {
    base: String,
    key: String,
    http: reqwest::Client,
}

impl Client {
    /// `url` is Bazarr's address (with its URL base, if it has one), `key` its API key.
    pub fn new(url: &str, key: &str) -> Self {
        Client {
            base: url.trim().trim_end_matches('/').to_string(),
            key: key.trim().to_string(),
            http: crate::tls::client()
                .timeout(Duration::from_secs(20))
                .build()
                .unwrap_or_default(),
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}/api/{path}", self.base)
    }

    async fn check(&self, what: &str, resp: reqwest::Response) -> Result<reqwest::Response> {
        let status = resp.status();
        if status.is_success() {
            return Ok(resp);
        }
        let body = resp.text().await.unwrap_or_default();
        let body = body.trim().trim_matches('"');
        match status.as_u16() {
            401 | 403 => bail!("Bazarr rejected the API key"),
            _ if body.is_empty() => bail!("Bazarr said {status} for {what}"),
            _ => bail!(
                "Bazarr said {status} for {what}: {}",
                body.chars().take(200).collect::<String>()
            ),
        }
    }

    async fn get(&self, path: &str, query: &[(&str, String)]) -> Result<Value> {
        let resp = self
            .http
            .get(self.url(path))
            .header("X-API-KEY", &self.key)
            .query(query)
            .send()
            .await
            .with_context(|| format!("couldn't reach Bazarr at {}", self.base))?;
        let resp = self.check(path, resp).await?;
        resp.json::<Value>()
            .await
            .with_context(|| format!("Bazarr's answer for {path} wasn't JSON"))
    }

    /// `get`, reusing the answer for a minute (the lists are large and the
    /// editor asks about one item after another).
    async fn get_cached(&self, path: &str, query: &[(&str, String)]) -> Result<Value> {
        let key = format!("{}|{path}|{query:?}", self.base);
        if let Some((at, v)) = cache().lock().unwrap_or_else(|p| p.into_inner()).get(&key) {
            if at.elapsed() < CACHE_TTL {
                return Ok(v.clone());
            }
        }
        let v = self.get(path, query).await?;
        cache()
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(key, (Instant::now(), v.clone()));
        Ok(v)
    }

    async fn patch(&self, path: &str, form: &[(&str, String)]) -> Result<()> {
        let resp = self
            .http
            .patch(self.url(path))
            .header("X-API-KEY", &self.key)
            // Bazarr reads these as query/form arguments; a query string is
            // accepted for PATCH everywhere it is used.
            .query(form)
            .send()
            .await
            .with_context(|| format!("couldn't reach Bazarr at {}", self.base))?;
        self.check(path, resp).await?;
        Ok(())
    }

    /// Bazarr's version, proving the address and key work.
    pub async fn version(&self) -> Result<String> {
        let v = self.get("system/status", &[]).await?;
        let data = v.get("data").unwrap_or(&v);
        Ok(data
            .get("bazarr_version")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string())
    }

    /// The languages Bazarr keeps subtitles in (see `parse_languages`).
    pub async fn languages(&self) -> Result<Vec<String>> {
        Ok(parse_languages(
            &self.get("system/languages/profiles", &[]).await?,
        ))
    }

    /// The movie or episode in Bazarr that `file_path` (a path in our
    /// library) is, or `None` if Bazarr doesn't have it or can't be told apart.
    pub async fn find(&self, file_path: &str) -> Result<Option<Target>> {
        let movies = self
            .get_cached("movies", &[("start", "0".into()), ("length", "-1".into())])
            .await?;
        let movies = items(&movies);
        if let Some(m) = best_match(movies, |m| tail_score(file_path, &text(m, "path"))) {
            if let Some(id) = int(m, "radarrId") {
                return Ok(Some(Target {
                    kind: Kind::Movie,
                    id,
                    series_id: None,
                    title: text(m, "title"),
                    path: text(m, "path"),
                    missing: needs(m),
                    profile_id: int(m, "profileId"),
                }));
            }
        }

        let series = self.get_cached("series", &[]).await?;
        let series = items(&series);
        let Some(show) = best_match(series, |s| series_score(&text(s, "path"), file_path)) else {
            return Ok(None);
        };
        let Some(series_id) = int(show, "sonarrSeriesId") else {
            return Ok(None);
        };
        // Episodes are never cached: they are what we are about to change.
        let episodes = self
            .get("episodes", &[("seriesid[]", series_id.to_string())])
            .await?;
        let episodes = items(&episodes);
        let Some(ep) = best_match(episodes, |e| tail_score(file_path, &text(e, "path"))) else {
            return Ok(None);
        };
        let Some(id) = int(ep, "sonarrEpisodeId") else {
            return Ok(None);
        };
        Ok(Some(Target {
            kind: Kind::Episode,
            id,
            series_id: Some(series_id),
            title: format!("{} - {}", text(show, "title"), text(ep, "title")),
            path: text(ep, "path"),
            missing: needs(ep),
            profile_id: int(ep, "profileId"),
        }))
    }

    /// Ask Bazarr to search for what it is missing for `t`. Returns how many
    /// languages it was asked about (0: it isn't missing anything).
    pub async fn search(&self, t: &Target) -> Result<usize> {
        if t.missing.is_empty() {
            return Ok(0);
        }
        match t.kind {
            Kind::Movie => {
                self.patch(
                    "movies",
                    &[
                        ("radarrid", t.id.to_string()),
                        ("action", "search-missing".into()),
                    ],
                )
                .await?;
            }
            Kind::Episode => {
                let series = t.series_id.context("episode without a series")?;
                for n in &t.missing {
                    self.patch(
                        "episodes/subtitles",
                        &[
                            ("seriesid", series.to_string()),
                            ("episodeid", t.id.to_string()),
                            ("language", n.code2.clone()),
                            ("forced", flag(n.forced)),
                            ("hi", flag(n.hi)),
                        ],
                    )
                    .await?;
                }
            }
        }
        // What it found shows up in the lists again.
        cache().lock().unwrap_or_else(|p| p.into_inner()).clear();
        Ok(t.missing.len())
    }

    /// Ask Bazarr to re-time `subtitle_file` (a file beside the video, named
    /// as it is on our side) against the video's audio. Bazarr does the work
    /// in the background; the corrected file replaces the old one.
    pub async fn sync(&self, t: &Target, subtitle_file: &str) -> Result<()> {
        self.subtitle_action(t, subtitle_file, "sync".to_string())
            .await
    }

    /// Ask Bazarr to move every line of `subtitle_file` (an `.srt` beside the
    /// video) by `offset_ms` (positive = later). Bazarr rewrites the file
    /// before it answers.
    pub async fn shift(&self, t: &Target, subtitle_file: &str, offset_ms: i64) -> Result<()> {
        if !subtitle_file.to_lowercase().ends_with(".srt") {
            bail!("Bazarr can only change the timing of .srt files");
        }
        if offset_ms == 0 || offset_ms.abs() > MAX_SHIFT_MS {
            bail!("the shift has to be between 1 ms and 10 minutes, earlier or later");
        }
        self.subtitle_action(t, subtitle_file, shift_action(offset_ms))
            .await
    }

    /// `PATCH subtitles` for a file beside the video: the part of "sync" and
    /// the timing mods that is the same.
    async fn subtitle_action(&self, t: &Target, subtitle_file: &str, action: String) -> Result<()> {
        let dir = components_dir(&t.path);
        let path = format!("{dir}{subtitle_file}");
        let stem = std::path::Path::new(&t.path)
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        let info = crate::langs::name_info(subtitle_file, &stem);
        let language = info
            .lang
            .map(str::to_string)
            .or_else(|| t.missing.first().map(|n| n.code2.clone()))
            .unwrap_or_else(|| "en".to_string());
        self.patch(
            "subtitles",
            &[
                ("action", action),
                ("language", language),
                ("path", path),
                (
                    "type",
                    match t.kind {
                        Kind::Movie => "movie".into(),
                        Kind::Episode => "episode".into(),
                    },
                ),
                ("id", t.id.to_string()),
                ("forced", flag(info.forced)),
                ("hi", flag(info.hi)),
            ],
        )
        .await
    }
}

/// The longest shift we will ask for: a subtitle that is further out than this
/// belongs to a different cut of the film, and re-timing it would not help.
const MAX_SHIFT_MS: i64 = 10 * 60 * 1000;

/// Bazarr's "adjust times" action for an offset in milliseconds, written the way
/// its mod parser reads it (no spaces; seconds and milliseconds keep the sign).
pub fn shift_action(offset_ms: i64) -> String {
    format!(
        "shift_offset(h=0,m=0,s={},ms={})",
        offset_ms / 1000,
        offset_ms % 1000
    )
}

fn flag(b: bool) -> String {
    if b { "True" } else { "False" }.to_string()
}

/// The folder part of a path, with its trailing separator and in the style the
/// path was written in (Bazarr may be on Windows), or "" for a bare name.
fn components_dir(path: &str) -> String {
    match path.rfind(['/', '\\']) {
        Some(i) => path[..=i].to_string(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn paths_match_by_their_ends_whatever_the_mount_point() {
        let ours = "/media/Movies/Paper Moon Heist (2021)/Paper Moon Heist (2021).mkv";
        let theirs = "/movies/Paper Moon Heist (2021)/Paper Moon Heist (2021).mkv"; // mounted elsewhere, same folders
        assert_eq!(tail_score(ours, theirs), 3);
        // Windows-style paths and case don't matter.
        assert_eq!(
            tail_score(
                ours,
                r"D:\Movies\paper moon heist (2021)\PAPER MOON HEIST (2021).MKV"
            ),
            3
        );
        // Different file name: no match at all, even if the folders agree.
        assert_eq!(
            tail_score(ours, "/movies/Paper Moon Heist (2021)/Other.mkv"),
            0
        );
    }

    #[test]
    fn a_shift_is_written_the_way_bazarr_parses_it() {
        assert_eq!(shift_action(1500), "shift_offset(h=0,m=0,s=1,ms=500)");
        assert_eq!(shift_action(-2250), "shift_offset(h=0,m=0,s=-2,ms=-250)");
        assert_eq!(shift_action(-40), "shift_offset(h=0,m=0,s=0,ms=-40)");
        assert!(!shift_action(1500).contains(' '));
    }

    #[test]
    fn best_match_refuses_a_tie() {
        let a = "/x/A/film.mkv";
        let list = ["/m/B/film.mkv", "/m/C/film.mkv"];
        assert_eq!(best_match(&list, |p| tail_score(a, p)), None);
        let list = ["/m/B/film.mkv", "/m/A/film.mkv"];
        assert_eq!(
            best_match(&list, |p| tail_score(a, p)),
            Some(&"/m/A/film.mkv")
        );
        assert_eq!(best_match(&["/m/B/other.mkv"], |p| tail_score(a, p)), None);
    }

    #[test]
    fn a_series_is_matched_by_its_folder_name() {
        let file = "/media/TV/Orbital Diner/Season 1/Orbital Diner - S01E01.mkv";
        assert!(series_score("/tv/Orbital Diner", file) >= 1);
        assert!(series_score("/data/tv/Orbital Diner", file) >= 1);
        assert_eq!(series_score("/tv/Other Show", file), 0);
        // The folder name must be a folder, not the file's own name.
        assert_eq!(series_score("/tv/Orbital Diner - S01E01.mkv", file), 0);
    }

    #[test]
    fn reads_the_languages_of_every_profile_once() {
        let profiles = json!([
            {"profileId": 1, "items": [{"language": "en", "hi": "False"}, {"language": "fr"}]},
            {"profileId": 2, "items": [{"language": "fr"}, {"language": "pt-BR"}]}
        ]);
        assert_eq!(parse_languages(&profiles), ["en", "fr", "pt"]);
        assert!(parse_languages(&json!({"data": []})).is_empty());
    }

    #[test]
    fn missing_languages_accept_bools_and_strings() {
        let v = json!({"missing_subtitles": [
            {"name": "English", "code2": "en", "code3": "eng", "forced": false, "hi": "True"},
            {"code2": "FR", "forced": "False"}
        ]});
        assert_eq!(
            needs(&v),
            vec![
                Need {
                    code2: "en".into(),
                    forced: false,
                    hi: true
                },
                Need {
                    code2: "fr".into(),
                    forced: false,
                    hi: false
                },
            ]
        );
    }

    // ---- against a stand-in Bazarr ---------------------------------------------

    use axum::{
        extract::{Query, State},
        http::{HeaderMap, StatusCode},
        routing::{get, patch},
        Json, Router,
    };
    use std::sync::Arc;

    type Calls = Arc<Mutex<Vec<String>>>;

    async fn serve(calls: Calls) -> String {
        async fn authed(h: &HeaderMap) -> bool {
            h.get("x-api-key").and_then(|v| v.to_str().ok()) == Some("sekrit")
        }
        let app = Router::new()
            .route(
                "/api/system/status",
                get(|h: HeaderMap| async move {
                    if !authed(&h).await {
                        return Err(StatusCode::UNAUTHORIZED);
                    }
                    Ok(Json(json!({"data": {"bazarr_version": "1.5.0"}})))
                }),
            )
            .route(
                "/api/movies",
                get(|State(c): State<Calls>| async move {
                    c.lock().unwrap().push("GET movies".into());
                    Json(json!({"data": [
                        {"radarrId": 7, "title": "Paper Moon Heist", "path": "/movies/Paper Moon Heist (2021)/Paper Moon Heist (2021).mkv",
                         "profileId": 1,
                         "missing_subtitles": [{"code2": "en", "forced": false, "hi": false}]}
                    ], "total": 1}))
                })
                .patch(
                    |State(c): State<Calls>, Query(q): Query<HashMap<String, String>>| async move {
                        c.lock().unwrap().push(format!(
                            "PATCH movies {} {}",
                            q["action"], q["radarrid"]
                        ));
                        StatusCode::NO_CONTENT
                    },
                ),
            )
            .route(
                "/api/series",
                get(|| async {
                    Json(json!({"data": [
                        {"sonarrSeriesId": 3, "title": "Orbital Diner", "path": "/tv/Orbital Diner"}
                    ]}))
                }),
            )
            .route(
                "/api/episodes",
                get(|| async {
                    Json(json!({"data": [
                        {"sonarrEpisodeId": 31, "sonarrSeriesId": 3, "title": "Zero-G Special",
                         "path": "/tv/Orbital Diner/Season 1/Orbital Diner - S01E01.mkv",
                         "missing_subtitles": [
                           {"code2": "en", "forced": false, "hi": false},
                           {"code2": "fr", "forced": false, "hi": true}]}
                    ]}))
                }),
            )
            .route(
                "/api/episodes/subtitles",
                patch(
                    |State(c): State<Calls>, Query(q): Query<HashMap<String, String>>| async move {
                        c.lock().unwrap().push(format!(
                            "PATCH episode {} {} {} hi={}",
                            q["seriesid"], q["episodeid"], q["language"], q["hi"]
                        ));
                        StatusCode::NO_CONTENT
                    },
                ),
            )
            .route(
                "/api/subtitles",
                patch(
                    |State(c): State<Calls>, Query(q): Query<HashMap<String, String>>| async move {
                        c.lock().unwrap().push(format!(
                            "SYNC {} {} {} {} {}",
                            q["action"], q["type"], q["id"], q["language"], q["path"]
                        ));
                        StatusCode::NO_CONTENT
                    },
                ),
            )
            .with_state(calls);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        format!("http://{addr}")
    }

    #[tokio::test]
    async fn checks_the_key_and_reports_the_version() {
        let url = serve(Calls::default()).await;
        assert_eq!(
            Client::new(&url, "sekrit").version().await.unwrap(),
            "1.5.0"
        );
        let err = Client::new(&url, "wrong").version().await.unwrap_err();
        assert!(err.to_string().contains("API key"), "{err}");
        let err = Client::new("http://127.0.0.1:1", "x")
            .version()
            .await
            .unwrap_err();
        assert!(err.to_string().contains("couldn't reach Bazarr"), "{err}");
    }

    #[tokio::test]
    async fn finds_a_movie_and_searches_for_what_is_missing() {
        let calls = Calls::default();
        let url = serve(calls.clone()).await;
        let c = Client::new(&url, "sekrit");
        let t = c
            .find("/media/Movies/Paper Moon Heist (2021)/Paper Moon Heist (2021).mkv")
            .await
            .unwrap()
            .expect("matched");
        assert_eq!((t.kind, t.id), (Kind::Movie, 7));
        assert_eq!(t.missing.len(), 1);
        assert_eq!(c.search(&t).await.unwrap(), 1);
        assert!(calls
            .lock()
            .unwrap()
            .contains(&"PATCH movies search-missing 7".to_string()));
        assert!(c
            .find("/media/Movies/Unknown/Unknown.mkv")
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn finds_an_episode_and_searches_each_missing_language() {
        let calls = Calls::default();
        let url = serve(calls.clone()).await;
        let c = Client::new(&url, "sekrit");
        let t = c
            .find("/media/TV/Orbital Diner/Season 1/Orbital Diner - S01E01.mkv")
            .await
            .unwrap()
            .expect("matched");
        assert_eq!((t.kind, t.id, t.series_id), (Kind::Episode, 31, Some(3)));
        assert_eq!(c.search(&t).await.unwrap(), 2);
        let log = calls.lock().unwrap().clone();
        assert!(
            log.contains(&"PATCH episode 3 31 en hi=False".to_string()),
            "{log:?}"
        );
        assert!(
            log.contains(&"PATCH episode 3 31 fr hi=True".to_string()),
            "{log:?}"
        );
    }

    #[tokio::test]
    async fn syncs_a_subtitle_by_its_path_as_bazarr_sees_it() {
        let calls = Calls::default();
        let url = serve(calls.clone()).await;
        let c = Client::new(&url, "sekrit");
        let t = c
            .find("/media/Movies/Paper Moon Heist (2021)/Paper Moon Heist (2021).mkv")
            .await
            .unwrap()
            .unwrap();
        c.sync(&t, "Paper Moon Heist (2021).en.srt").await.unwrap();
        let log = calls.lock().unwrap().clone();
        assert!(
            log.contains(
                &"SYNC sync movie 7 en /movies/Paper Moon Heist (2021)/Paper Moon Heist (2021).en.srt".to_string()
            ),
            "{log:?}"
        );
    }

    #[tokio::test]
    async fn shifts_an_srt_and_refuses_what_bazarr_cannot_or_should_not() {
        let calls = Calls::default();
        let url = serve(calls.clone()).await;
        let c = Client::new(&url, "sekrit");
        let t = c
            .find("/media/Movies/Paper Moon Heist (2021)/Paper Moon Heist (2021).mkv")
            .await
            .unwrap()
            .unwrap();
        c.shift(&t, "Paper Moon Heist (2021).en.srt", -1500)
            .await
            .unwrap();
        let log = calls.lock().unwrap().clone();
        let want = "SYNC shift_offset(h=0,m=0,s=-1,ms=-500) movie 7 en \
                    /movies/Paper Moon Heist (2021)/Paper Moon Heist (2021).en.srt";
        assert!(log.contains(&want.to_string()), "{log:?}");
        // Not an .srt, nothing to shift, or an absurd shift: never sent.
        let before = calls.lock().unwrap().len();
        assert!(c
            .shift(&t, "Paper Moon Heist (2021).en.ass", 500)
            .await
            .is_err());
        assert!(c
            .shift(&t, "Paper Moon Heist (2021).en.srt", 0)
            .await
            .is_err());
        assert!(c
            .shift(&t, "Paper Moon Heist (2021).en.srt", 11 * 60 * 1000)
            .await
            .is_err());
        assert_eq!(calls.lock().unwrap().len(), before);
    }

    #[tokio::test]
    async fn a_movie_that_is_missing_nothing_needs_no_search() {
        let t = Target {
            kind: Kind::Movie,
            id: 1,
            series_id: None,
            title: "x".into(),
            path: "/x.mkv".into(),
            missing: vec![],
            profile_id: None,
        };
        assert_eq!(
            Client::new("http://127.0.0.1:1", "k")
                .search(&t)
                .await
                .unwrap(),
            0
        );
    }
}
