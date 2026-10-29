//! The media server the app indexes from: Plex or Jellyfin. Both are reduced
//! to the same two operations -- list libraries, enumerate a library's movies
//! or episodes with their on-disk path -- so everything downstream (indexer,
//! path mapping, search, export) is server-agnostic. Files are read from
//! mounted volumes either way; the server only says which files exist.

use crate::jellyfin::JellyfinClient;
use crate::plex::PlexClient;
use anyhow::Result;

/// Which kind of media server is configured.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ServerKind {
    #[default]
    Plex,
    Jellyfin,
}

impl ServerKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ServerKind::Plex => "plex",
            ServerKind::Jellyfin => "jellyfin",
        }
    }

    /// Display name, for messages.
    pub fn label(self) -> &'static str {
        match self {
            ServerKind::Plex => "Plex",
            ServerKind::Jellyfin => "Jellyfin",
        }
    }

    /// Unknown or missing values mean Plex -- the only server there was
    /// before this setting existed, so existing installs keep working.
    pub fn parse(s: Option<&str>) -> ServerKind {
        match s.map(str::trim) {
            Some("jellyfin") => ServerKind::Jellyfin,
            _ => ServerKind::Plex,
        }
    }
}

/// A library on the media server.
#[derive(Debug, Clone)]
pub struct Section {
    /// Server-specific id used to list the library's items.
    pub key: String,
    /// `"movie"` | `"show"` | anything else (music, photos...: not indexed).
    pub kind: String,
    pub title: String,
    /// Root folder path(s) the server scans for this library (used to
    /// suggest and check path mappings).
    pub locations: Vec<String>,
}

/// A single indexable item (movie or episode), flattened for our needs.
#[derive(Debug, Clone)]
pub struct Item {
    /// The server's stable id for the item (Plex ratingKey, Jellyfin Id).
    pub rating_key: String,
    pub kind: String,
    pub title: String,
    pub show: Option<String>,
    pub season: Option<i64>,
    pub episode: Option<i64>,
    pub year: Option<i64>,
    pub file: Option<String>,
    /// Plex-only: the part key for its download API (unused; direct file
    /// mounts are the only access mode).
    pub part_key: Option<String>,
    pub duration_ms: i64,
    /// The library (section) this item belongs to.
    pub library: String,
}

/// A subtitle candidate from a server's online search, in one shape for both
/// servers. `key` is opaque: pass it back to download.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SubtitleCandidate {
    pub key: String,
    pub title: String,
    pub language: Option<String>,
    pub format: Option<String>,
    pub provider: Option<String>,
    pub hearing_impaired: bool,
    pub forced: bool,
    /// A perfect match for this file: Jellyfin's `IsHashMatch` (file hash) or
    /// Plex's `perfectMatch`. The most reliable kind of match.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hash_match: Option<bool>,
    /// Jellyfin only: how often it has been downloaded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub downloads: Option<i64>,
    /// Plex only: its match score for the result (python-plexapi documents it
    /// as the subtitle's download count).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score: Option<i64>,
    /// A longer description than `title`, when the server gives one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extended_title: Option<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub ai_translated: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub machine_translated: bool,
    /// Everything else the server said about this subtitle, as text, under
    /// the server's own attribute names (the UI shows these on request).
    #[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub details: std::collections::BTreeMap<String, String>,
}

/// A scalar JSON value as display text; objects, arrays, nulls and empty
/// strings have nothing worth showing.
pub fn scalar_text(v: &serde_json::Value) -> Option<String> {
    match v {
        serde_json::Value::String(s) if !s.trim().is_empty() => Some(s.trim().to_string()),
        serde_json::Value::Number(n) => Some(n.to_string()),
        serde_json::Value::Bool(b) => Some(if *b { "yes" } else { "no" }.to_string()),
        _ => None,
    }
}

/// What the media server said when asked to download a subtitle. Kept so the
/// log can show what the server actually answered: a download that is
/// "accepted" but never shows up needs this to be diagnosed.
#[derive(Debug, Clone, serde::Serialize)]
pub struct DownloadAck {
    pub status: u16,
    /// The start of the response body (servers mostly send nothing).
    pub body: String,
    pub elapsed_ms: u64,
}

/// One subtitle stream the media server lists for an item, as it reports it.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ServerSubtitle {
    /// Display name the server gives it.
    pub name: String,
    pub language: Option<String>,
    pub format: Option<String>,
    /// A separate subtitle (downloaded or sidecar) rather than one inside the
    /// video file.
    pub external: bool,
    /// Where the server says the file is, when it says (Jellyfin reports an
    /// external subtitle's path; Plex gives a stream key instead).
    pub path: Option<String>,
    /// The server's own handle for it (Plex `/library/streams/<id>`).
    pub key: Option<String>,
    pub forced: bool,
    pub hearing_impaired: bool,
}

/// The media server answered a subtitle request with this HTTP status. Kept as
/// a typed error so the API can pick a sensible status of its own instead of
/// calling every upstream refusal a 500.
#[derive(Debug)]
pub struct UpstreamStatus {
    pub status: u16,
    pub message: String,
}

impl std::fmt::Display for UpstreamStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for UpstreamStatus {}

/// Whether `id` could be an item id on this kind of server: Plex rating keys
/// are numbers, Jellyfin ids are GUIDs (32 hex digits, or 36 with dashes).
pub fn valid_item_id(kind: ServerKind, id: &str) -> bool {
    match kind {
        ServerKind::Plex => {
            !id.is_empty() && id.len() <= 20 && id.bytes().all(|b| b.is_ascii_digit())
        }
        ServerKind::Jellyfin => {
            matches!(id.len(), 32 | 36) && id.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-')
        }
    }
}

pub enum MediaServer {
    Plex(PlexClient),
    Jellyfin(JellyfinClient),
}

impl MediaServer {
    pub fn kind(&self) -> ServerKind {
        match self {
            MediaServer::Plex(_) => ServerKind::Plex,
            MediaServer::Jellyfin(_) => ServerKind::Jellyfin,
        }
    }

    pub async fn sections(&self) -> Result<Vec<Section>> {
        match self {
            MediaServer::Plex(c) => c.sections().await,
            MediaServer::Jellyfin(c) => c.sections().await,
        }
    }

    /// The server's reported version string.
    pub async fn server_version(&self) -> Result<String> {
        match self {
            MediaServer::Plex(c) => c.server_version().await,
            MediaServer::Jellyfin(c) => c.server_version().await,
        }
    }

    /// Search the server's online subtitle sources. `language` is a code as
    /// the caller supplied it; each server maps it to what it expects.
    pub async fn subtitle_search(
        &self,
        item_id: &str,
        language: &str,
    ) -> Result<Vec<SubtitleCandidate>> {
        match self {
            MediaServer::Plex(c) => c.subtitle_search(item_id, language).await,
            MediaServer::Jellyfin(c) => c.subtitle_search(item_id, language).await,
        }
    }

    pub async fn subtitle_download(&self, item_id: &str, key: &str) -> Result<DownloadAck> {
        match self {
            MediaServer::Plex(c) => c.subtitle_download(item_id, key).await,
            MediaServer::Jellyfin(c) => c.subtitle_download(item_id, key).await,
        }
    }

    /// The subtitle streams the server lists for this item right now.
    pub async fn item_subtitle_streams(&self, item_id: &str) -> Result<Vec<ServerSubtitle>> {
        match self {
            MediaServer::Plex(c) => c.item_subtitle_streams(item_id).await,
            MediaServer::Jellyfin(c) => c.item_subtitle_streams(item_id).await,
        }
    }

    /// The raw bytes of an external subtitle the server stores itself, by its
    /// stream key. Only Plex keeps subtitles somewhere other than a file.
    pub async fn subtitle_stream_bytes(&self, key: &str) -> Result<Vec<u8>> {
        match self {
            MediaServer::Plex(c) => c.subtitle_stream_bytes(key).await,
            MediaServer::Jellyfin(_) => anyhow::bail!("Jellyfin subtitles are read from files"),
        }
    }

    pub async fn items(&self, section: &Section) -> Result<Vec<Item>> {
        match self {
            MediaServer::Plex(c) => c.items(section).await,
            MediaServer::Jellyfin(c) => c.items(section).await,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subtitle_urls_rely_on_urlencoding_escaping_reserved_characters() {
        // Subtitle keys and language codes are spliced into request URLs
        // with `urlencoding::encode`; these are the behaviours that matter.
        let enc = |s| urlencoding::encode(s).into_owned();
        assert_eq!(enc("en"), "en");
        assert_eq!(enc("/library/streams/1"), "%2Flibrary%2Fstreams%2F1");
        assert_eq!(enc("a b&c?d#e"), "a%20b%26c%3Fd%23e");
        assert_eq!(enc("é"), "%C3%A9");
        assert_eq!(enc("a-b_c.d~e"), "a-b_c.d~e"); // unreserved characters pass through
    }

    #[test]
    fn item_ids_are_checked_per_server_kind() {
        let guid = "0f8fad5bd9cb469fa16570867728950e";
        let dashed = "0f8fad5b-d9cb-469f-a165-70867728950e";
        assert!(valid_item_id(ServerKind::Plex, "12345"));
        assert!(!valid_item_id(ServerKind::Plex, guid));
        assert!(!valid_item_id(ServerKind::Plex, ""));
        assert!(valid_item_id(ServerKind::Jellyfin, guid));
        assert!(valid_item_id(ServerKind::Jellyfin, dashed));
        for bad in [
            "12345",
            "",
            "../../etc/passwd",
            "0f8fad5bd9cb469fa16570867728950g",
            &format!("{guid}0"),
        ] {
            assert!(!valid_item_id(ServerKind::Jellyfin, bad), "{bad}");
        }
    }

    #[test]
    fn kind_parsing_defaults_to_plex_so_existing_installs_are_unaffected() {
        assert_eq!(ServerKind::parse(None), ServerKind::Plex);
        assert_eq!(ServerKind::parse(Some("plex")), ServerKind::Plex);
        assert_eq!(ServerKind::parse(Some("jellyfin")), ServerKind::Jellyfin);
        assert_eq!(ServerKind::parse(Some(" jellyfin ")), ServerKind::Jellyfin);
        assert_eq!(ServerKind::parse(Some("emby")), ServerKind::Plex);
        for k in [ServerKind::Plex, ServerKind::Jellyfin] {
            assert_eq!(ServerKind::parse(Some(k.as_str())), k);
        }
    }
}
