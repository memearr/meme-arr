//! Minimal Plex Media Server client: list libraries and enumerate movies /
//! episodes with the on-disk file path and part key we need. We request JSON
//! (`Accept: application/json`) and authenticate with `X-Plex-Token`.

/// A subtitle file is a few hundred KB; refuse anything absurd.
const MAX_SUBTITLE_BYTES: usize = 8 * 1024 * 1024;

use crate::mediaserver::{DownloadAck, Item, Section, ServerSubtitle, SubtitleCandidate};
use anyhow::{Context, Result};
use reqwest::Client;
use serde::Deserialize;

pub struct PlexClient {
    base_url: String,
    token: String,
    http: Client,
}

// ---- JSON shapes ---------------------------------------------------------

#[derive(Debug, Deserialize)]
struct Envelope<T> {
    #[serde(rename = "MediaContainer")]
    container: T,
}

#[derive(Debug, Deserialize)]
struct SectionsContainer {
    #[serde(rename = "Directory", default)]
    directory: Vec<RawSection>,
}

#[derive(Debug, Deserialize)]
struct RawSection {
    key: String,
    #[serde(rename = "type")]
    kind: String,
    title: String,
    #[serde(rename = "Location", default)]
    location: Vec<RawLocation>,
}

#[derive(Debug, Deserialize)]
struct RawLocation {
    #[serde(default)]
    path: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MetadataContainer {
    #[serde(rename = "Metadata", default)]
    metadata: Vec<RawItem>,
}

#[derive(Debug, Deserialize)]
struct RawItem {
    #[serde(rename = "ratingKey")]
    rating_key: String,
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    title: String,
    #[serde(rename = "grandparentTitle", default)]
    grandparent_title: Option<String>,
    #[serde(rename = "parentIndex", default)]
    parent_index: Option<i64>,
    #[serde(default)]
    index: Option<i64>,
    #[serde(default)]
    year: Option<i64>,
    #[serde(default)]
    duration: Option<i64>,
    #[serde(rename = "Media", default)]
    media: Vec<RawMedia>,
}

#[derive(Debug, Deserialize)]
struct RawMedia {
    #[serde(rename = "Part", default)]
    part: Vec<RawPart>,
}

#[derive(Debug, Deserialize)]
struct RawPart {
    #[serde(default)]
    file: Option<String>,
    #[serde(default)]
    key: Option<String>,
}

/// Plex added subtitle search (OpenSubtitles.com-backed) in this server
/// version; older servers don't have the endpoint.
/// <https://support.plex.tv/articles/subtitle-search/>
pub const MIN_SUBTITLE_SEARCH_VERSION: &str = "1.13.8.5395";

/// Whether a server of this version supports subtitle search. `None` means
/// the version string couldn't be understood -- callers should then try the
/// request anyway rather than refuse a server that may well support it.
pub fn supports_subtitle_search(version: &str) -> Option<bool> {
    let v = softver::Version::find(version)?;
    let min = softver::Version::parse(MIN_SUBTITLE_SEARCH_VERSION).ok()?;
    Some(v >= min)
}

#[derive(Debug, Deserialize)]
struct IdentityContainer {
    #[serde(default)]
    version: Option<String>,
}

// ---- /library/metadata/{id}: which subtitle streams Plex thinks an item has ----

#[derive(Debug, Deserialize)]
struct ItemStreamsContainer {
    #[serde(rename = "Metadata", default)]
    metadata: Vec<ItemStreamsMeta>,
}

#[derive(Debug, Deserialize)]
struct ItemStreamsMeta {
    #[serde(rename = "Media", default)]
    media: Vec<ItemStreamsMedia>,
}

#[derive(Debug, Deserialize)]
struct ItemStreamsMedia {
    #[serde(rename = "Part", default)]
    part: Vec<ItemStreamsPart>,
}

#[derive(Debug, Deserialize)]
struct ItemStreamsPart {
    #[serde(rename = "Stream", default)]
    stream: Vec<ItemStream>,
}

#[derive(Debug, Deserialize)]
struct ItemStream {
    #[serde(rename = "streamType", default)]
    stream_type: serde_json::Value,
    #[serde(default)]
    key: Option<String>,
    #[serde(default)]
    file: Option<String>,
    #[serde(default)]
    codec: Option<String>,
    #[serde(default)]
    format: Option<String>,
    #[serde(default)]
    language: Option<String>,
    #[serde(rename = "displayTitle", default)]
    display_title: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    forced: serde_json::Value,
    #[serde(rename = "hearingImpaired", default)]
    hearing_impaired: serde_json::Value,
}

/// The subtitle streams (type 3) across all of an item's media parts.
fn subtitle_streams_from(c: ItemStreamsContainer) -> Vec<ServerSubtitle> {
    c.metadata
        .into_iter()
        .flat_map(|m| m.media)
        .flat_map(|m| m.part)
        .flat_map(|p| p.stream)
        .filter(|s| as_i64(&s.stream_type) == Some(3))
        .map(|s| {
            let non_empty =
                |v: Option<String>| v.map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
            ServerSubtitle {
                name: non_empty(s.display_title)
                    .or_else(|| non_empty(s.title))
                    .unwrap_or_else(|| "Subtitle".into()),
                language: non_empty(s.language),
                format: non_empty(s.format).or_else(|| non_empty(s.codec)),
                external: s.key.as_deref().is_some_and(|k| !k.is_empty()),
                path: non_empty(s.file),
                key: non_empty(s.key),
                forced: truthy(&s.forced),
                hearing_impaired: truthy(&s.hearing_impaired),
            }
        })
        .collect()
}

#[derive(Debug, Deserialize)]
struct StreamsContainer {
    #[serde(rename = "Stream", default)]
    stream: Vec<RawStream>,
}

#[derive(Debug, Deserialize)]
struct RawStream {
    #[serde(default)]
    key: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(rename = "displayTitle", default)]
    display_title: Option<String>,
    #[serde(rename = "extendedDisplayTitle", default)]
    extended_display_title: Option<String>,
    #[serde(default)]
    language: Option<String>,
    #[serde(default)]
    format: Option<String>,
    #[serde(default)]
    codec: Option<String>,
    #[serde(rename = "providerTitle", default)]
    provider_title: Option<String>,
    // Plex sends these as 0/1, "0"/"1" or booleans depending on endpoint.
    #[serde(rename = "hearingImpaired", default)]
    hearing_impaired: serde_json::Value,
    #[serde(default)]
    forced: serde_json::Value,
    /// python-plexapi documents these two for on-demand (searched) subtitles:
    /// `perfectMatch` -- a perfect match for this file -- and `score`, the
    /// match score (which it describes as the download count).
    #[serde(rename = "perfectMatch", default)]
    perfect_match: serde_json::Value,
    #[serde(default)]
    score: serde_json::Value,
    /// Every other attribute Plex sent, kept so the UI can show it.
    #[serde(flatten)]
    extra: std::collections::HashMap<String, serde_json::Value>,
}

fn truthy(v: &serde_json::Value) -> bool {
    match v {
        serde_json::Value::Bool(b) => *b,
        serde_json::Value::Number(n) => n.as_i64().is_some_and(|n| n != 0),
        serde_json::Value::String(s) => s == "1" || s.eq_ignore_ascii_case("true"),
        _ => false,
    }
}

/// Plex may send a number as a JSON number or as a string.
fn as_i64(v: &serde_json::Value) -> Option<i64> {
    match v {
        serde_json::Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)),
        serde_json::Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// Attributes already shown in their own column/badge, or just internal
/// bookkeeping -- not worth repeating under "details".
const NOT_DETAILS: &[&str] = &[
    "key",
    "id",
    "index",
    "streamType",
    "selected",
    "default",
    "userID",
    "transient",
    "decision",
    "location",
];

fn candidate_from(raw: RawStream) -> Option<SubtitleCandidate> {
    let key = raw.key.filter(|k| !k.is_empty())?;
    let non_empty = |s: Option<String>| s.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());

    // The most descriptive name Plex gave: its `title` (for searched
    // subtitles that is usually the release/file name), else the longer
    // display text, else the short one.
    let (title, display, extended) = (
        non_empty(raw.title),
        non_empty(raw.display_title),
        non_empty(raw.extended_display_title),
    );
    let primary = title
        .clone()
        .or_else(|| extended.clone())
        .or_else(|| display.clone())
        .unwrap_or_else(|| "Subtitle".into());
    let extended_title = extended.clone().filter(|e| *e != primary);

    let mut details = std::collections::BTreeMap::new();
    for (name, value) in [("title", &title), ("displayTitle", &display)] {
        if let Some(v) = value.as_ref().filter(|v| **v != primary) {
            details.insert(name.to_string(), v.clone());
        }
    }
    for (name, value) in &raw.extra {
        if NOT_DETAILS.contains(&name.as_str()) {
            continue;
        }
        if let Some(text) = crate::mediaserver::scalar_text(value) {
            details.insert(name.clone(), text);
        }
    }

    Some(SubtitleCandidate {
        title: primary,
        extended_title,
        language: raw.language,
        format: raw.format.or(raw.codec),
        provider: raw.provider_title,
        hearing_impaired: truthy(&raw.hearing_impaired),
        forced: truthy(&raw.forced),
        // Only claim a perfect match when Plex actually said so either way.
        hash_match: (!raw.perfect_match.is_null()).then(|| truthy(&raw.perfect_match)),
        score: as_i64(&raw.score),
        downloads: None,
        ai_translated: false,
        machine_translated: false,
        details,
        key,
    })
}

/// A subtitle key from a search result must be a plain server-relative path
/// before we send it back to Plex -- it's appended to the server URL.
pub fn valid_subtitle_key(key: &str) -> bool {
    key.starts_with('/')
        && !key.starts_with("//")
        && !key.contains("..")
        && !key.contains(['?', '#', '\\'])
}

impl PlexClient {
    /// The server's version (`GET /identity`, which needs no token).
    pub async fn server_version(&self) -> Result<String> {
        let env: Envelope<IdentityContainer> = self.get_json("/identity").await?;
        env.container
            .version
            .filter(|v| !v.is_empty())
            .context("Plex didn't report a version")
    }

    /// Search Plex's online sources for subtitles for an item.
    pub async fn subtitle_search(
        &self,
        rating_key: &str,
        language: &str,
    ) -> Result<Vec<SubtitleCandidate>> {
        let path = format!(
            "/library/metadata/{rating_key}/subtitles?language={}&hearingImpaired=0&forced=0",
            urlencoding::encode(language)
        );
        let env: Envelope<StreamsContainer> = self.get_json(&path).await?;
        Ok(env
            .container
            .stream
            .into_iter()
            .filter_map(candidate_from)
            .collect())
    }

    /// Ask Plex to download a searched subtitle and attach it to the item.
    /// Plex does this asynchronously: success means "accepted", not "done".
    pub async fn subtitle_download(&self, rating_key: &str, key: &str) -> Result<DownloadAck> {
        let url = format!(
            "{}/library/metadata/{rating_key}/subtitles?key={}&X-Plex-Token={}",
            self.base_url,
            urlencoding::encode(key),
            self.token
        );
        let started = std::time::Instant::now();
        let resp = self
            .http
            .put(&url)
            .send()
            .await
            .map_err(reqwest::Error::without_url)
            .context("Plex subtitle download request failed")?
            .error_for_status()
            .map_err(reqwest::Error::without_url)
            .context("Plex rejected the subtitle download")?;
        let status = resp.status().as_u16();
        let body = resp.text().await.unwrap_or_default();
        Ok(DownloadAck {
            status,
            body: body.chars().take(300).collect(),
            elapsed_ms: started.elapsed().as_millis() as u64,
        })
    }

    /// The subtitle streams Plex lists for an item (`GET /library/metadata/{id}`).
    /// A stream with a `key` is an external subtitle (one Plex downloaded or
    /// found beside the file); one without is embedded in the video.
    pub async fn item_subtitle_streams(&self, rating_key: &str) -> Result<Vec<ServerSubtitle>> {
        let env: Envelope<ItemStreamsContainer> = self
            .get_json(&format!("/library/metadata/{rating_key}"))
            .await?;
        Ok(subtitle_streams_from(env.container))
    }

    /// The text of an external subtitle Plex stores for an item
    /// (`GET /library/streams/{id}`). Plex keeps subtitles it downloaded or
    /// that were uploaded in its own database, so this is the only way to read
    /// them. `key` must be a stream key as listed by `item_subtitle_streams`.
    pub async fn subtitle_stream_bytes(&self, key: &str) -> Result<Vec<u8>> {
        let id = key.strip_prefix("/library/streams/").unwrap_or("");
        if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric()) {
            anyhow::bail!("not a Plex subtitle stream key");
        }
        let url = format!(
            "{}/library/streams/{id}?X-Plex-Token={}",
            self.base_url, self.token
        );
        let resp = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(reqwest::Error::without_url)
            .context("Plex subtitle stream request failed")?
            .error_for_status()
            .map_err(reqwest::Error::without_url)
            .context("Plex refused to serve the subtitle stream")?;
        let body = resp.bytes().await.map_err(reqwest::Error::without_url)?;
        if body.len() > MAX_SUBTITLE_BYTES {
            anyhow::bail!(
                "subtitle stream is unexpectedly large ({} bytes)",
                body.len()
            );
        }
        Ok(body.to_vec())
    }

    pub fn new(base_url: impl Into<String>, token: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            token: token.into(),
            http: Client::new(),
        }
    }

    async fn get_json<T: for<'de> Deserialize<'de>>(&self, path_and_query: &str) -> Result<T> {
        let sep = if path_and_query.contains('?') {
            '&'
        } else {
            '?'
        };
        let url = format!(
            "{}{}{}X-Plex-Token={}",
            self.base_url, path_and_query, sep, self.token
        );
        let resp = self
            .http
            .get(&url)
            .header("Accept", "application/json")
            .send()
            .await
            // The URL carries X-Plex-Token as a query parameter, and reqwest
            // prints the whole URL in its error text -- which ends up in the
            // Activity log, health report and logs. Strip it.
            .map_err(reqwest::Error::without_url)
            .with_context(|| format!("Plex request failed: {path_and_query}"))?
            .error_for_status()
            .map_err(reqwest::Error::without_url)
            .with_context(|| format!("Plex returned an error for {path_and_query}"))?;
        let body = resp.bytes().await.map_err(reqwest::Error::without_url)?;
        serde_json::from_slice(&body)
            .with_context(|| format!("could not parse Plex JSON for {path_and_query}"))
    }

    pub async fn sections(&self) -> Result<Vec<Section>> {
        let env: Envelope<SectionsContainer> = self.get_json("/library/sections").await?;
        Ok(env
            .container
            .directory
            .into_iter()
            .map(|d| Section {
                key: d.key,
                kind: d.kind,
                title: d.title,
                locations: d.location.into_iter().filter_map(|l| l.path).collect(),
            })
            .collect())
    }

    /// All movies in a movie section, or all episodes in a show section
    /// (`?type=4` asks Plex to flatten straight to episodes).
    pub async fn items(&self, section: &Section) -> Result<Vec<Item>> {
        let path = if section.kind == "show" {
            format!("/library/sections/{}/all?type=4", section.key)
        } else {
            format!("/library/sections/{}/all", section.key)
        };
        let env: Envelope<MetadataContainer> = self.get_json(&path).await?;
        Ok(env
            .container
            .metadata
            .into_iter()
            .map(|raw| flatten_item(raw, &section.title))
            .collect())
    }

    /// Absolute URL to download a part's bytes over the Plex API (fallback path,
    /// used when a file is not reachable on a mounted volume).
    #[allow(dead_code)]
    pub fn part_download_url(&self, part_key: &str) -> String {
        format!(
            "{}{}?download=1&X-Plex-Token={}",
            self.base_url, part_key, self.token
        )
    }
}

fn flatten_item(raw: RawItem, library: &str) -> Item {
    let (file, part_key) = raw
        .media
        .into_iter()
        .flat_map(|m| m.part.into_iter())
        .next()
        .map(|p| (p.file, p.key))
        .unwrap_or((None, None));

    Item {
        rating_key: raw.rating_key,
        kind: raw.kind,
        title: raw.title,
        show: raw.grandparent_title,
        season: raw.parent_index,
        episode: raw.index,
        year: raw.year,
        file,
        part_key,
        duration_ms: raw.duration.unwrap_or(0),
        library: library.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sections() {
        let json = r#"{"MediaContainer":{"Directory":[
            {"key":"1","type":"movie","title":"Movies"},
            {"key":"2","type":"show","title":"TV Shows"}
        ]}}"#;
        let env: Envelope<SectionsContainer> = serde_json::from_str(json).unwrap();
        assert_eq!(env.container.directory.len(), 2);
        assert_eq!(env.container.directory[1].kind, "show");
    }

    #[test]
    fn flattens_episode_with_part() {
        let json = r#"{"MediaContainer":{"Metadata":[{
            "ratingKey":"555","type":"episode","title":"Pilot",
            "grandparentTitle":"Breaking Bad","parentIndex":1,"index":1,"year":2008,
            "duration":2820000,
            "Media":[{"Part":[{"file":"/volume1/media/BB/S01E01.mkv","key":"/library/parts/1/123/file.mkv"}]}]
        }]}}"#;
        let env: Envelope<MetadataContainer> = serde_json::from_str(json).unwrap();
        let item = flatten_item(
            env.container.metadata.into_iter().next().unwrap(),
            "TV Shows",
        );
        assert_eq!(item.rating_key, "555");
        assert_eq!(item.library, "TV Shows");
        assert_eq!(item.show.as_deref(), Some("Breaking Bad"));
        assert_eq!(item.season, Some(1));
        assert_eq!(item.episode, Some(1));
        assert_eq!(item.file.as_deref(), Some("/volume1/media/BB/S01E01.mkv"));
        assert_eq!(item.duration_ms, 2_820_000);
    }

    #[test]
    fn item_without_media_is_tolerated() {
        let json = r#"{"MediaContainer":{"Metadata":[{
            "ratingKey":"1","type":"movie","title":"Ghost"
        }]}}"#;
        let env: Envelope<MetadataContainer> = serde_json::from_str(json).unwrap();
        let item = flatten_item(env.container.metadata.into_iter().next().unwrap(), "Movies");
        assert!(item.file.is_none());
        assert_eq!(item.duration_ms, 0);
    }

    #[test]
    fn subtitle_search_version_gate() {
        let ok = supports_subtitle_search;
        assert_eq!(ok("1.13.8.5395"), Some(true)); // exactly the minimum
        assert_eq!(ok("1.13.8.5394"), Some(false));
        assert_eq!(ok("1.13.7.9999"), Some(false));
        assert_eq!(ok("1.9.0.0"), Some(false)); // numeric, not lexical
        assert_eq!(ok("1.41.3.9314-a0bfb8370"), Some(true)); // real Plex format
        assert_eq!(ok("1.13.8.5394-deadbeef"), Some(false)); // suffix can't rescue it
        assert_eq!(ok("2.0.0.1"), Some(true));
        assert_eq!(ok("1.13.9"), Some(true)); // fewer parts than the minimum
        assert_eq!(ok("1.41.3.9314.1"), Some(true)); // more parts
        assert_eq!(ok("v1.41.3.9314"), Some(true));
        assert_eq!(ok("nope"), None); // unknown: caller decides, not us
    }

    #[test]
    fn subtitle_results_parse_with_loose_flag_types() {
        let json = r#"{"MediaContainer":{"Stream":[
            {"key":"/library/streams/1","displayTitle":"English (SRT)","language":"English","format":"srt",
             "providerTitle":"OpenSubtitles","hearingImpaired":"1","forced":0},
            {"key":"/library/streams/2","title":"x","codec":"ass","hearingImpaired":false,"forced":true},
            {"title":"no key, dropped"}
        ]}}"#;
        let env: Envelope<StreamsContainer> = serde_json::from_str(json).unwrap();
        let c: Vec<_> = env
            .container
            .stream
            .into_iter()
            .filter_map(candidate_from)
            .collect();
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].title, "English (SRT)");
        assert!(c[0].hearing_impaired && !c[0].forced);
        assert_eq!(c[0].provider.as_deref(), Some("OpenSubtitles"));
        assert_eq!(c[1].format.as_deref(), Some("ass"));
        assert!(!c[1].hearing_impaired && c[1].forced);
    }

    #[test]
    fn a_rich_search_result_surfaces_score_perfect_match_and_the_rest() {
        let json = r#"{"MediaContainer":{"Stream":[{
            "id":"900","key":"/library/streams/900","streamType":"3","index":"4",
            "title":"Show.S01E02.1080p.WEB-DL-GRP","displayTitle":"English (SRT)",
            "extendedDisplayTitle":"English (SRT) from OpenSubtitles","language":"English",
            "languageCode":"eng","languageTag":"en","format":"srt","providerTitle":"OpenSubtitles",
            "perfectMatch":"1","score":"4200","canAutoSync":"1","sourceKey":"opensubtitles:abc",
            "hearingImpaired":"0","forced":"0","userID":"7","selected":"0"
        }]}}"#;
        let env: Envelope<StreamsContainer> = serde_json::from_str(json).unwrap();
        let c = candidate_from(env.container.stream.into_iter().next().unwrap()).unwrap();
        assert_eq!(c.title, "Show.S01E02.1080p.WEB-DL-GRP"); // the release name beats "English"
        assert_eq!(
            c.extended_title.as_deref(),
            Some("English (SRT) from OpenSubtitles")
        );
        assert_eq!(c.hash_match, Some(true));
        assert_eq!(c.score, Some(4200));
        assert_eq!(c.provider.as_deref(), Some("OpenSubtitles"));
        // Anything else Plex sent is kept, under its own name...
        assert_eq!(
            c.details.get("languageCode").map(String::as_str),
            Some("eng")
        );
        assert_eq!(
            c.details.get("sourceKey").map(String::as_str),
            Some("opensubtitles:abc")
        );
        assert_eq!(c.details.get("canAutoSync").map(String::as_str), Some("1"));
        assert_eq!(
            c.details.get("displayTitle").map(String::as_str),
            Some("English (SRT)")
        );
        // ...but not what has its own column, or internal bookkeeping.
        for gone in [
            "key",
            "id",
            "index",
            "userID",
            "selected",
            "score",
            "perfectMatch",
            "format",
            "providerTitle",
        ] {
            assert!(
                !c.details.contains_key(gone),
                "{gone} should not be in details"
            );
        }
    }

    #[test]
    fn perfect_match_is_only_claimed_when_plex_says_so() {
        let one = |extra: &str| {
            let json = format!(r#"{{"key":"/library/streams/1","title":"t"{extra}}}"#);
            candidate_from(serde_json::from_str(&json).unwrap()).unwrap()
        };
        assert_eq!(one("").hash_match, None); // field absent: say nothing
        assert_eq!(one(r#","perfectMatch":0"#).hash_match, Some(false));
        assert_eq!(one(r#","perfectMatch":true"#).hash_match, Some(true));
        assert_eq!(one(r#","score":12.0"#).score, Some(12));
        assert_eq!(one(r#","score":"junk""#).score, None);
    }

    #[test]
    fn only_plain_server_paths_are_accepted_as_subtitle_keys() {
        assert!(valid_subtitle_key("/library/streams/123"));
        for bad in [
            "",
            "library/x",
            "//evil.example/x",
            "/a/../b",
            "/a?x=1",
            "/a#b",
            "/a\\b",
            "http://evil/x",
        ] {
            assert!(!valid_subtitle_key(bad), "{bad}");
        }
    }

    #[test]
    fn item_subtitle_streams_split_external_from_embedded() {
        let json = r#"{"MediaContainer":{"Metadata":[{"Media":[{"Part":[{"file":"/data/tv/S01E01.mkv","Stream":[
            {"streamType":1,"codec":"h264"},
            {"streamType":2,"codec":"aac","language":"English"},
            {"streamType":3,"codec":"subrip","language":"English","displayTitle":"English (SRT)","format":"srt"},
            {"streamType":"3","key":"/library/streams/77","codec":"srt","format":"srt","language":"French",
             "displayTitle":"French (SRT External)","forced":"1","hearingImpaired":0,"file":"/meta/Subtitles/fr.srt"}
        ]}]}]}]}}"#;
        let env: Envelope<ItemStreamsContainer> = serde_json::from_str(json).unwrap();
        let subs = subtitle_streams_from(env.container);
        assert_eq!(
            subs.len(),
            2,
            "only subtitle streams (type 3, number or string)"
        );
        assert!(
            !subs[0].external && subs[0].key.is_none(),
            "embedded: no key"
        );
        assert_eq!(subs[0].name, "English (SRT)");
        assert!(subs[1].external, "a stream with a key is external");
        assert_eq!(subs[1].key.as_deref(), Some("/library/streams/77"));
        assert_eq!(subs[1].path.as_deref(), Some("/meta/Subtitles/fr.srt"));
        assert!(subs[1].forced && !subs[1].hearing_impaired);
    }

    #[test]
    fn an_item_with_no_media_has_no_subtitle_streams() {
        let env: Envelope<ItemStreamsContainer> =
            serde_json::from_str(r#"{"MediaContainer":{"Metadata":[{}]}}"#).unwrap();
        assert!(subtitle_streams_from(env.container).is_empty());
    }
}
