//! Minimal Jellyfin client: list libraries ("virtual folders") and enumerate
//! their movies / episodes with each file's on-disk path. Authenticates with
//! an API key (Dashboard -> API Keys) sent as
//! `Authorization: MediaBrowser Token="<key>"`.
//!
//! Endpoints and field names are from Jellyfin's OpenAPI spec
//! (`GET /Library/VirtualFolders`, `GET /Items`, `GET /System/Info`).

use crate::mediaserver::{
    DownloadAck, Item, Section, ServerSubtitle, SubtitleCandidate, UpstreamStatus,
};
use anyhow::{Context, Result};
use reqwest::Client;
use serde::Deserialize;
use std::time::Duration;

/// Page size for `GET /Items` -- big enough to keep a 100k-item library to a
/// few hundred requests, small enough that a page is a modest response.
const PAGE: usize = 500;
/// 1 ms is 10,000 Jellyfin "ticks" (100 ns each).
const TICKS_PER_MS: i64 = 10_000;

pub struct JellyfinClient {
    base_url: String,
    api_key: String,
    http: Client,
}

// ---- JSON shapes (only the fields we read) --------------------------------

#[derive(Debug, Deserialize)]
struct VirtualFolder {
    #[serde(rename = "Name", default)]
    name: String,
    #[serde(rename = "Locations", default)]
    locations: Vec<String>,
    #[serde(rename = "CollectionType", default)]
    collection_type: Option<String>,
    #[serde(rename = "ItemId", default)]
    item_id: String,
}

#[derive(Debug, Deserialize)]
struct ItemsPage {
    #[serde(rename = "Items", default)]
    items: Vec<RawItem>,
    #[serde(rename = "TotalRecordCount", default)]
    total: usize,
}

#[derive(Debug, Deserialize)]
struct RawItem {
    #[serde(rename = "Id")]
    id: String,
    #[serde(rename = "Type", default)]
    kind: String,
    #[serde(rename = "Name", default)]
    name: String,
    #[serde(rename = "SeriesName", default)]
    series_name: Option<String>,
    #[serde(rename = "ParentIndexNumber", default)]
    season: Option<i64>,
    #[serde(rename = "IndexNumber", default)]
    episode: Option<i64>,
    #[serde(rename = "ProductionYear", default)]
    year: Option<i64>,
    #[serde(rename = "Path", default)]
    path: Option<String>,
    #[serde(rename = "RunTimeTicks", default)]
    run_time_ticks: Option<i64>,
}

// ---- /Items?Ids=..&Fields=MediaSources: the subtitle streams of one item ----

#[derive(Debug, Deserialize)]
struct StreamsPage {
    #[serde(rename = "Items", default)]
    items: Vec<StreamsItem>,
}

#[derive(Debug, Deserialize)]
struct StreamsItem {
    #[serde(rename = "MediaSources", default)]
    sources: Vec<StreamsSource>,
}

#[derive(Debug, Deserialize)]
struct StreamsSource {
    #[serde(rename = "MediaStreams", default)]
    streams: Vec<MediaStream>,
}

#[derive(Debug, Deserialize)]
struct MediaStream {
    #[serde(rename = "Type", default)]
    kind: String,
    #[serde(rename = "IsExternal", default)]
    external: bool,
    #[serde(rename = "Path", default)]
    path: Option<String>,
    #[serde(rename = "Language", default)]
    language: Option<String>,
    #[serde(rename = "Codec", default)]
    codec: Option<String>,
    #[serde(rename = "DisplayTitle", default)]
    display_title: Option<String>,
    #[serde(rename = "Title", default)]
    title: Option<String>,
    #[serde(rename = "IsForced", default)]
    forced: bool,
    #[serde(rename = "IsHearingImpaired", default)]
    hearing_impaired: bool,
}

fn subtitle_streams_from(p: StreamsPage) -> Vec<ServerSubtitle> {
    p.items
        .into_iter()
        .flat_map(|i| i.sources)
        .flat_map(|s| s.streams)
        .filter(|s| s.kind == "Subtitle")
        .map(|s| {
            let non_empty =
                |v: Option<String>| v.map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
            ServerSubtitle {
                name: non_empty(s.display_title)
                    .or_else(|| non_empty(s.title))
                    .unwrap_or_else(|| "Subtitle".into()),
                language: non_empty(s.language),
                format: non_empty(s.codec),
                external: s.external,
                path: non_empty(s.path),
                key: None,
                forced: s.forced,
                hearing_impaired: s.hearing_impaired,
            }
        })
        .collect()
}

#[derive(Debug, Deserialize)]
struct RawSubtitle {
    #[serde(rename = "Id", default)]
    id: String,
    #[serde(rename = "ProviderName", default)]
    provider: Option<String>,
    #[serde(rename = "Name", default)]
    name: Option<String>,
    #[serde(rename = "Format", default)]
    format: Option<String>,
    #[serde(rename = "ThreeLetterISOLanguageName", default)]
    language: Option<String>,
    #[serde(rename = "DownloadCount", default)]
    downloads: Option<i64>,
    #[serde(rename = "IsHashMatch", default)]
    hash_match: Option<bool>,
    #[serde(rename = "Forced", default)]
    forced: Option<bool>,
    #[serde(rename = "HearingImpaired", default)]
    hearing_impaired: Option<bool>,
    #[serde(rename = "Author", default)]
    author: Option<String>,
    #[serde(rename = "Comment", default)]
    comment: Option<String>,
    #[serde(rename = "CommunityRating", default)]
    community_rating: Option<f64>,
    #[serde(rename = "FrameRate", default)]
    frame_rate: Option<f64>,
    #[serde(rename = "DateCreated", default)]
    date_created: Option<String>,
    #[serde(rename = "AiTranslated", default)]
    ai_translated: Option<bool>,
    #[serde(rename = "MachineTranslated", default)]
    machine_translated: Option<bool>,
}

fn candidate_from(raw: RawSubtitle) -> Option<SubtitleCandidate> {
    if raw.id.is_empty() {
        return None;
    }
    // The extra facts Jellyfin gives (all fields of its RemoteSubtitleInfo
    // schema), under that schema's names, for the UI's "details".
    let mut details = std::collections::BTreeMap::new();
    let mut put = |name: &str, v: Option<String>| {
        if let Some(v) = v.map(|v| v.trim().to_string()).filter(|v| !v.is_empty()) {
            details.insert(name.to_string(), v);
        }
    };
    put("Author", raw.author.clone());
    put("Comment", raw.comment.clone());
    put(
        "CommunityRating",
        raw.community_rating.map(|r| r.to_string()),
    );
    put("FrameRate", raw.frame_rate.map(|r| r.to_string()));
    put("DateCreated", raw.date_created.clone());
    Some(SubtitleCandidate {
        extended_title: None,
        score: None,
        ai_translated: raw.ai_translated.unwrap_or(false),
        machine_translated: raw.machine_translated.unwrap_or(false),
        details,
        title: raw
            .name
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| "Subtitle".into()),
        language: raw.language,
        format: raw.format,
        provider: raw.provider,
        hearing_impaired: raw.hearing_impaired.unwrap_or(false),
        forced: raw.forced.unwrap_or(false),
        hash_match: raw.hash_match,
        downloads: raw.downloads,
        key: raw.id,
    })
}

/// The three-letter code Jellyfin's subtitle search takes (.NET's
/// `ThreeLetterISOLanguageName`, e.g. `eng`, `fra`, `deu`). Accepts those
/// directly, or a two-letter code (`en`, `pt-BR`) for common languages;
/// `None` for a two-letter code not in the table.
///
/// The accepted *format* of Jellyfin's `language` path parameter isn't
/// specified in its API docs ("The language of the subtitles"); three-letter
/// codes are what Jellyfin's own web client sends.
pub fn language_code(lang: &str) -> Option<String> {
    let primary = lang.split('-').next().unwrap_or(lang).to_ascii_lowercase();
    if !primary.bytes().all(|b| b.is_ascii_lowercase()) {
        return None;
    }
    if primary.len() == 3 {
        return Some(primary);
    }
    let three = match primary.as_str() {
        "en" => "eng",
        "es" => "spa",
        "fr" => "fra",
        "de" => "deu",
        "it" => "ita",
        "pt" => "por",
        "nl" => "nld",
        "sv" => "swe",
        "da" => "dan",
        "no" | "nb" => "nor",
        "fi" => "fin",
        "pl" => "pol",
        "ru" => "rus",
        "ja" => "jpn",
        "ko" => "kor",
        "zh" => "zho",
        "ar" => "ara",
        "tr" => "tur",
        "cs" => "ces",
        "el" => "ell",
        "he" => "heb",
        "hi" => "hin",
        "hu" => "hun",
        "ro" => "ron",
        "uk" => "ukr",
        "vi" => "vie",
        "th" => "tha",
        "id" => "ind",
        _ => return None,
    };
    Some(three.to_string())
}

impl JellyfinClient {
    /// The server's version, from the unauthenticated public info endpoint.
    pub async fn server_version(&self) -> Result<String> {
        #[derive(Deserialize)]
        struct Info {
            #[serde(rename = "Version", default)]
            version: String,
        }
        let info: Info = self.get_json("/System/Info/Public").await?;
        if info.version.is_empty() {
            anyhow::bail!("Jellyfin didn't report a version");
        }
        Ok(info.version)
    }

    /// Search the subtitle providers installed on the server. An empty list
    /// is normal when no provider plugin (e.g. OpenSubtitles) is configured.
    pub async fn subtitle_search(
        &self,
        item_id: &str,
        language: &str,
    ) -> Result<Vec<SubtitleCandidate>> {
        let lang = language_code(language).with_context(|| {
            format!("unknown language {language:?}: use a three-letter code like eng")
        })?;
        let path = format!(
            "/Items/{item_id}/RemoteSearch/Subtitles/{}",
            urlencoding::encode(&lang)
        );
        let raw: Vec<RawSubtitle> = self.get_json_subtitles(&path).await?;
        Ok(raw.into_iter().filter_map(candidate_from).collect())
    }

    /// Ask Jellyfin to download a searched subtitle and attach it to the item.
    pub async fn subtitle_download(&self, item_id: &str, subtitle_id: &str) -> Result<DownloadAck> {
        let url = format!(
            "{}/Items/{item_id}/RemoteSearch/Subtitles/{}",
            self.base_url,
            urlencoding::encode(subtitle_id)
        );
        let started = std::time::Instant::now();
        let resp = self
            .http
            .post(&url)
            .header("Authorization", auth_header(&self.api_key))
            .send()
            .await
            .map_err(reqwest::Error::without_url)
            .context("Jellyfin subtitle download request failed")?;
        let status = resp.status();
        check_subtitle_status(status)?;
        let body = resp.text().await.unwrap_or_default();
        Ok(DownloadAck {
            status: status.as_u16(),
            body: body.chars().take(300).collect(),
            elapsed_ms: started.elapsed().as_millis() as u64,
        })
    }

    /// The subtitle streams Jellyfin lists for an item. For an external
    /// subtitle (including one it just downloaded) Jellyfin reports the file's
    /// path, which shows where it was saved.
    pub async fn item_subtitle_streams(&self, item_id: &str) -> Result<Vec<ServerSubtitle>> {
        let page: StreamsPage = self
            .get_json(&format!("/Items?Ids={item_id}&Fields=MediaSources"))
            .await?;
        Ok(subtitle_streams_from(page))
    }

    /// `get_json` for subtitle endpoints, which have their own meanings for
    /// 403 (missing subtitle-management permission) and 404 (unknown item).
    async fn get_json_subtitles<T: for<'de> Deserialize<'de>>(&self, path: &str) -> Result<T> {
        let resp = self
            .http
            .get(format!("{}{}", self.base_url, path))
            .header("Authorization", auth_header(&self.api_key))
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(reqwest::Error::without_url)
            .context("Jellyfin subtitle search request failed")?;
        check_subtitle_status(resp.status())?;
        let body = resp.bytes().await.map_err(reqwest::Error::without_url)?;
        serde_json::from_slice(&body).context("could not parse Jellyfin's subtitle results")
    }

    pub fn new(base_url: impl Into<String>, api_key: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            api_key: api_key.into(),
            http: Client::builder()
                .timeout(Duration::from_secs(60))
                .build()
                .unwrap_or_default(),
        }
    }

    async fn get_json<T: for<'de> Deserialize<'de>>(&self, path_and_query: &str) -> Result<T> {
        let url = format!("{}{}", self.base_url, path_and_query);
        let resp = self
            .http
            .get(&url)
            .header("Authorization", auth_header(&self.api_key))
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(reqwest::Error::without_url)
            .with_context(|| format!("Jellyfin request failed: {path_and_query}"))?;
        let status = resp.status();
        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            anyhow::bail!(
                "Jellyfin rejected the API key ({status}) -- create one under Dashboard → API Keys"
            );
        }
        let resp = resp
            .error_for_status()
            .map_err(reqwest::Error::without_url)
            .with_context(|| format!("Jellyfin returned an error for {path_and_query}"))?;
        let body = resp.bytes().await.map_err(reqwest::Error::without_url)?;
        serde_json::from_slice(&body)
            .with_context(|| format!("could not parse Jellyfin JSON for {path_and_query}"))
    }

    pub async fn sections(&self) -> Result<Vec<Section>> {
        let folders: Vec<VirtualFolder> = self.get_json("/Library/VirtualFolders").await?;
        Ok(folders.into_iter().map(section_from).collect())
    }

    /// All movies in a movie library, or all episodes in a TV library,
    /// fetched page by page.
    pub async fn items(&self, section: &Section) -> Result<Vec<Item>> {
        let mut out = Vec::new();
        loop {
            let path = items_query(section, out.len(), PAGE);
            let page: ItemsPage = self.get_json(&path).await?;
            let got = page.items.len();
            out.extend(
                page.items
                    .into_iter()
                    .map(|raw| flatten_item(raw, &section.title)),
            );
            // Stop on an empty page (guards against a server that ignores
            // StartIndex) or once everything reported has been read.
            if got == 0 || out.len() >= page.total {
                return Ok(out);
            }
        }
    }
}

fn check_subtitle_status(status: reqwest::StatusCode) -> Result<()> {
    use reqwest::StatusCode as S;
    let message = match status {
        s if s.is_success() => return Ok(()),
        S::UNAUTHORIZED => "Jellyfin rejected the API key (401)".to_string(),
        S::FORBIDDEN => "Jellyfin denied the request (403): subtitle search/download needs the SubtitleManagement permission".to_string(),
        S::NOT_FOUND => "Jellyfin doesn't know that item (404)".to_string(),
        other => format!("Jellyfin returned {other} for the subtitle request"),
    };
    Err(UpstreamStatus {
        status: status.as_u16(),
        message,
    }
    .into())
}

/// `Authorization: MediaBrowser Token="<key>"`, with a client name so the key
/// shows up recognizably in Jellyfin's activity/devices views.
fn auth_header(api_key: &str) -> String {
    format!(r#"MediaBrowser Client="meme-arr", Token="{api_key}""#)
}

fn section_from(f: VirtualFolder) -> Section {
    let kind = match f.collection_type.as_deref() {
        Some("movies") => "movie",
        Some("tvshows") => "show",
        // Music, books, photos, mixed content...: listed (so the UI can show
        // them) but never indexed.
        Some(other) => other,
        None => "mixed",
    };
    Section {
        key: f.item_id,
        kind: kind.to_string(),
        title: f.name,
        locations: f.locations,
    }
}

/// The `GET /Items` request for one page of a library.
fn items_query(section: &Section, start: usize, limit: usize) -> String {
    let item_type = if section.kind == "show" {
        "Episode"
    } else {
        "Movie"
    };
    format!(
        "/Items?ParentId={}&Recursive=true&IncludeItemTypes={item_type}&Fields=Path\
         &SortBy=SortName&StartIndex={start}&Limit={limit}&EnableTotalRecordCount=true",
        section.key
    )
}

fn flatten_item(raw: RawItem, library: &str) -> Item {
    Item {
        rating_key: raw.id,
        kind: raw.kind.to_lowercase(),
        title: raw.name,
        show: raw.series_name,
        season: raw.season,
        episode: raw.episode,
        year: raw.year,
        file: raw.path,
        part_key: None,
        duration_ms: raw.run_time_ticks.unwrap_or(0) / TICKS_PER_MS,
        library: library.to_string(),
    }
}

/// Check the URL and API key work: `GET /System/Info` requires authorization
/// (unlike `/System/Info/Public`), so it proves the key, not just the host.
/// Returns the server's name.
pub async fn test_connection(base_url: &str, api_key: &str) -> Result<String> {
    #[derive(Deserialize)]
    struct Info {
        #[serde(rename = "ServerName", default)]
        name: String,
    }
    let info: Info = JellyfinClient::new(base_url, api_key)
        .get_json("/System/Info")
        .await?;
    Ok(info.name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn libraries_map_collection_types_and_keep_folders() {
        let json = r#"[
            {"Name":"Movies","Locations":["/data/movies"],"CollectionType":"movies","ItemId":"aaa"},
            {"Name":"Shows","Locations":["/data/tv","D:\\Anime"],"CollectionType":"tvshows","ItemId":"bbb"},
            {"Name":"Music","Locations":["/data/music"],"CollectionType":"music","ItemId":"ccc"},
            {"Name":"Misc","Locations":[],"ItemId":"ddd"}
        ]"#;
        let folders: Vec<VirtualFolder> = serde_json::from_str(json).unwrap();
        let s: Vec<Section> = folders.into_iter().map(section_from).collect();
        assert_eq!((s[0].kind.as_str(), s[0].key.as_str()), ("movie", "aaa"));
        assert_eq!(s[1].kind, "show");
        assert_eq!(s[1].locations, ["/data/tv", "D:\\Anime"]);
        assert_eq!(s[2].kind, "music"); // listed, but the indexer only takes movie/show
        assert_eq!(s[3].kind, "mixed");
    }

    #[test]
    fn episodes_and_movies_flatten_with_ticks_converted_to_ms() {
        let json = r#"{"Items":[
            {"Id":"e1","Type":"Episode","Name":"Pilot","SeriesName":"Breaking Bad",
             "ParentIndexNumber":1,"IndexNumber":1,"ProductionYear":2008,
             "Path":"/data/tv/Breaking Bad/S01E01.mkv","RunTimeTicks":29400000000},
            {"Id":"m1","Type":"Movie","Name":"Dune","ProductionYear":2021,"Path":"/data/movies/Dune.mkv"},
            {"Id":"m2","Type":"Movie","Name":"No path"}
        ],"TotalRecordCount":3,"StartIndex":0}"#;
        let page: ItemsPage = serde_json::from_str(json).unwrap();
        assert_eq!(page.total, 3);
        let items: Vec<Item> = page
            .items
            .into_iter()
            .map(|r| flatten_item(r, "Lib"))
            .collect();
        assert_eq!(items[0].kind, "episode");
        assert_eq!(items[0].show.as_deref(), Some("Breaking Bad"));
        assert_eq!((items[0].season, items[0].episode), (Some(1), Some(1)));
        assert_eq!(items[0].duration_ms, 2_940_000); // 49 minutes
        assert_eq!(items[0].library, "Lib");
        assert_eq!(items[1].kind, "movie");
        assert_eq!(items[1].duration_ms, 0);
        assert_eq!(items[2].file, None); // tolerated, like a Plex item with no media
    }

    #[test]
    fn items_query_picks_the_type_and_pages() {
        let movies = Section {
            key: "k1".into(),
            kind: "movie".into(),
            title: "M".into(),
            locations: vec![],
        };
        let shows = Section {
            kind: "show".into(),
            key: "k2".into(),
            ..movies.clone()
        };
        let q = items_query(&movies, 0, 500);
        assert!(
            q.contains("ParentId=k1")
                && q.contains("IncludeItemTypes=Movie")
                && q.contains("StartIndex=0")
        );
        let q = items_query(&shows, 1000, 500);
        assert!(
            q.contains("IncludeItemTypes=Episode")
                && q.contains("StartIndex=1000")
                && q.contains("Limit=500")
        );
        assert!(q.contains("Fields=Path") && q.contains("Recursive=true"));
    }

    #[test]
    fn auth_header_carries_the_key_in_the_mediabrowser_scheme() {
        let h = auth_header("abc123");
        assert!(h.starts_with("MediaBrowser ") && h.contains(r#"Token="abc123""#));
    }

    #[tokio::test]
    async fn an_unreachable_server_error_never_contains_the_api_key() {
        let err = JellyfinClient::new("http://127.0.0.1:9", "SECRETKEY")
            .sections()
            .await
            .unwrap_err();
        assert!(!format!("{err:#}").contains("SECRETKEY"));
    }

    #[test]
    fn subtitle_results_map_with_optional_fields_missing() {
        let json = r#"[
            {"Id":"abc_123","ProviderName":"Open Subtitles","Name":"Dune.2021.1080p","Format":"srt",
             "ThreeLetterISOLanguageName":"eng","DownloadCount":4200,"IsHashMatch":true,
             "Forced":false,"HearingImpaired":true},
            {"Id":"x","Name":""},
            {"Name":"no id, dropped"}
        ]"#;
        let raw: Vec<RawSubtitle> = serde_json::from_str(json).unwrap();
        let c: Vec<_> = raw.into_iter().filter_map(candidate_from).collect();
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].key, "abc_123");
        assert_eq!(c[0].provider.as_deref(), Some("Open Subtitles"));
        assert_eq!((c[0].hash_match, c[0].downloads), (Some(true), Some(4200)));
        assert!(c[0].hearing_impaired && !c[0].forced);
        assert_eq!(c[1].title, "Subtitle"); // blank name falls back
        assert_eq!(c[1].hash_match, None);
    }

    #[test]
    fn jellyfin_extras_become_details_and_flags() {
        let json = r#"[{"Id":"s1","Name":"Pilot","Author":"someone","Comment":"  synced to WEB  ",
            "CommunityRating":8.5,"FrameRate":23.976,"DateCreated":"2024-05-01T10:00:00Z",
            "AiTranslated":true,"MachineTranslated":false,"IsHashMatch":true}]"#;
        let raw: Vec<RawSubtitle> = serde_json::from_str(json).unwrap();
        let c = candidate_from(raw.into_iter().next().unwrap()).unwrap();
        assert!(c.ai_translated && !c.machine_translated);
        assert_eq!(c.details.get("Author").map(String::as_str), Some("someone"));
        assert_eq!(
            c.details.get("Comment").map(String::as_str),
            Some("synced to WEB")
        ); // trimmed
        assert_eq!(
            c.details.get("CommunityRating").map(String::as_str),
            Some("8.5")
        );
        assert_eq!(
            c.details.get("FrameRate").map(String::as_str),
            Some("23.976")
        );
        assert!(c.details.contains_key("DateCreated"));
        // A candidate with none of the extras has no details at all.
        let bare: Vec<RawSubtitle> = serde_json::from_str(r#"[{"Id":"s2"}]"#).unwrap();
        let b = candidate_from(bare.into_iter().next().unwrap()).unwrap();
        assert!(b.details.is_empty() && !b.ai_translated);
    }

    #[test]
    fn language_codes_map_to_three_letters() {
        assert_eq!(language_code("en").as_deref(), Some("eng"));
        assert_eq!(language_code("pt-BR").as_deref(), Some("por"));
        assert_eq!(language_code("FR").as_deref(), Some("fra"));
        assert_eq!(language_code("eng").as_deref(), Some("eng"));
        assert_eq!(language_code("ZHO").as_deref(), Some("zho"));
        assert_eq!(language_code("xx"), None); // unknown two-letter
        assert_eq!(language_code("e1"), None);
        assert_eq!(language_code(""), None);
    }

    #[test]
    fn subtitle_statuses_have_specific_messages() {
        use reqwest::StatusCode as S;
        assert!(check_subtitle_status(S::OK).is_ok());
        assert!(check_subtitle_status(S::NO_CONTENT).is_ok());
        assert!(check_subtitle_status(S::FORBIDDEN)
            .unwrap_err()
            .to_string()
            .contains("SubtitleManagement"));
        assert!(check_subtitle_status(S::NOT_FOUND)
            .unwrap_err()
            .to_string()
            .contains("item"));
        assert!(check_subtitle_status(S::UNAUTHORIZED)
            .unwrap_err()
            .to_string()
            .contains("API key"));
        assert!(check_subtitle_status(S::SERVICE_UNAVAILABLE).is_err());
        let e = check_subtitle_status(S::FORBIDDEN).unwrap_err();
        assert_eq!(
            e.downcast_ref::<UpstreamStatus>().map(|u| u.status),
            Some(403)
        );
    }

    #[test]
    fn item_subtitle_streams_report_where_an_external_one_was_saved() {
        let json = r#"{"Items":[{"MediaSources":[{"MediaStreams":[
            {"Type":"Video","Codec":"h264"},
            {"Type":"Subtitle","Codec":"subrip","Language":"eng","DisplayTitle":"English - SRT","IsExternal":false},
            {"Type":"Subtitle","Codec":"srt","Language":"fra","Title":"Fr","IsExternal":true,
             "Path":"/config/data/subtitles/abc/fr.srt","IsForced":true}
        ]}]}]}"#;
        let page: StreamsPage = serde_json::from_str(json).unwrap();
        let subs = subtitle_streams_from(page);
        assert_eq!(subs.len(), 2, "video streams are not subtitles");
        assert!(!subs[0].external && subs[0].path.is_none());
        assert!(subs[1].external && subs[1].forced);
        assert_eq!(
            subs[1].path.as_deref(),
            Some("/config/data/subtitles/abc/fr.srt")
        );
        assert_eq!(
            subs[1].name, "Fr",
            "falls back to Title when there's no DisplayTitle"
        );
    }

    #[test]
    fn an_item_with_no_sources_has_no_subtitle_streams() {
        let page: StreamsPage = serde_json::from_str(r#"{"Items":[{}]}"#).unwrap();
        assert!(subtitle_streams_from(page).is_empty());
        let none: StreamsPage = serde_json::from_str("{}").unwrap();
        assert!(subtitle_streams_from(none).is_empty());
    }
}
