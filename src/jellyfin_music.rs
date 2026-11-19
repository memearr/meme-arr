//! Songs in Jellyfin's music libraries: search them, fetch one to use under a clip, and get its
//! cover. Uses the same API key as the rest of the app's Jellyfin features.
//!
//! What is used: `GET /Items?IncludeItemTypes=Audio` (search), `GET /Items/{id}/Download` (the
//! original file, not a transcode) and `GET /Items/{id}/Images/Primary` (the cover). A song's id
//! here is `jf-` and Jellyfin's own id for it.

use crate::jellyfin::{auth_header, JellyfinClient};
use crate::local_music::JELLYFIN_PREFIX;
use crate::navidrome::{Track, MAX_COVER_BYTES};
use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

/// 1 s is 10,000,000 Jellyfin "ticks" (100 ns each).
const TICKS_PER_S: i64 = 10_000_000;

#[derive(Debug, Deserialize)]
struct SongPage {
    #[serde(rename = "Items", default)]
    items: Vec<RawSong>,
}

#[derive(Debug, Deserialize)]
struct RawSong {
    #[serde(rename = "Id")]
    id: String,
    #[serde(rename = "Name", default)]
    name: String,
    #[serde(rename = "Artists", default)]
    artists: Vec<String>,
    #[serde(rename = "AlbumArtist", default)]
    album_artist: Option<String>,
    #[serde(rename = "Album", default)]
    album: Option<String>,
    #[serde(rename = "RunTimeTicks", default)]
    ticks: Option<i64>,
    #[serde(rename = "ImageTags", default)]
    image_tags: HashMap<String, String>,
    #[serde(rename = "AlbumId", default)]
    album_id: Option<String>,
    #[serde(rename = "AlbumPrimaryImageTag", default)]
    album_image_tag: Option<String>,
}

/// The songs in a `GET /Items` answer. An item without an id is dropped.
fn parse_songs(page: SongPage) -> Vec<Track> {
    page.items
        .into_iter()
        .filter(|s| !s.id.is_empty())
        .map(|s| {
            // the song's own picture if it has one, else its album's
            let cover = if s.image_tags.contains_key("Primary") {
                Some(s.id.as_str())
            } else if s.album_image_tag.is_some() {
                s.album_id.as_deref()
            } else {
                None
            };
            let artist = s
                .artists
                .first()
                .cloned()
                .or(s.album_artist.clone())
                .unwrap_or_default();
            Track {
                id: format!("{JELLYFIN_PREFIX}{}", s.id),
                title: s.name.clone(),
                artist,
                album: s.album.clone().unwrap_or_default(),
                duration_s: (s.ticks.unwrap_or(0).max(0) / TICKS_PER_S).min(u32::MAX as i64) as u32,
                cover_art: cover
                    .map(|c| format!("{JELLYFIN_PREFIX}{c}"))
                    .unwrap_or_default(),
            }
        })
        .collect()
}

/// Jellyfin's own id for a song or picture id of ours (`jf-…`).
fn jellyfin_id(id: &str) -> Result<&str> {
    match id.strip_prefix(JELLYFIN_PREFIX) {
        Some(rest) if crate::music::valid_id(rest) => Ok(rest),
        _ => bail!("that isn't a Jellyfin song"),
    }
}

impl JellyfinClient {
    /// Songs whose name, artist or album match `query` (at most `limit`).
    pub async fn search_songs(&self, query: &str, limit: u32) -> Result<Vec<Track>> {
        let mut q = reqwest::Url::parse("http://x/").context("bad url")?;
        q.query_pairs_mut()
            .append_pair("IncludeItemTypes", "Audio")
            .append_pair("Recursive", "true")
            .append_pair("SearchTerm", query)
            .append_pair("Limit", &limit.to_string())
            .append_pair("Fields", "Album,AlbumArtist");
        let path = format!("/Items?{}", q.query().unwrap_or_default());
        let page: SongPage = self.get_json(&path).await?;
        Ok(parse_songs(page))
    }

    async fn get_bytes(&self, path: &str, timeout: Duration) -> Result<reqwest::Response> {
        let resp = self
            .http
            .get(format!("{}{}", self.base_url, path))
            .header("Authorization", auth_header(&self.api_key))
            .timeout(timeout)
            .send()
            .await
            .map_err(reqwest::Error::without_url)
            .context("couldn't reach Jellyfin")?;
        let status = resp.status();
        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            bail!(
                "Jellyfin rejected the API key ({status}) -- create one under Dashboard → API Keys"
            );
        }
        if !status.is_success() {
            bail!("Jellyfin answered {status}");
        }
        Ok(resp)
    }

    /// Download song `id` (a [`Track::id`]: the original file) to `dest`, giving up past
    /// `max_bytes`. Written to a side file and renamed, so a failure leaves nothing behind.
    pub async fn fetch_song(&self, id: &str, dest: &Path, max_bytes: u64) -> Result<()> {
        let jid = jellyfin_id(id)?;
        let resp = self
            .get_bytes(&format!("/Items/{jid}/Download"), Duration::from_secs(300))
            .await?;
        let kind = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        if kind.contains("json") || kind.contains("html") || kind.contains("xml") {
            bail!("Jellyfin sent no audio");
        }
        crate::music::save_stream(resp, dest, max_bytes, "Jellyfin").await
    }

    /// The cover picture `id` (a [`Track::cover_art`]) scaled to about `size` pixels.
    pub async fn song_cover(&self, id: &str, size: u32) -> Result<(String, Vec<u8>)> {
        let jid = jellyfin_id(id)?;
        let resp = self
            .get_bytes(
                &format!("/Items/{jid}/Images/Primary?maxWidth={size}"),
                Duration::from_secs(20),
            )
            .await?;
        let kind = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        if !kind.starts_with("image/") {
            bail!("Jellyfin sent no picture");
        }
        let bytes = resp.bytes().await.context("the picture didn't arrive")?;
        if bytes.is_empty() || bytes.len() > MAX_COVER_BYTES {
            bail!("the picture is empty or too big");
        }
        Ok((kind, bytes.to_vec()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(json: &str) -> SongPage {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn songs_are_read_with_their_artist_album_length_and_cover() {
        let got = parse_songs(page(
            r#"{"Items": [
                {"Id": "aa11", "Name": "Theme", "Artists": ["Band", "Guest"], "Album": "LP",
                 "RunTimeTicks": 1875000000, "ImageTags": {"Primary": "t"}},
                {"Id": "bb22", "Name": "Track 2", "AlbumArtist": "Various", "Album": "Mix",
                 "AlbumId": "cc33", "AlbumPrimaryImageTag": "x"},
                {"Id": "dd44", "Name": "Bare"},
                {"Id": "", "Name": "no id"}
            ]}"#,
        ));
        assert_eq!(got.len(), 3);
        assert_eq!(
            got[0],
            Track {
                id: "jf-aa11".into(),
                title: "Theme".into(),
                artist: "Band".into(),
                album: "LP".into(),
                duration_s: 187,
                cover_art: "jf-aa11".into(),
            }
        );
        // no artist of its own: the album's; no picture of its own: the album's
        assert_eq!(
            (got[1].artist.as_str(), got[1].cover_art.as_str()),
            ("Various", "jf-cc33")
        );
        assert_eq!((got[2].duration_s, got[2].cover_art.as_str()), (0, ""));
    }

    #[test]
    fn nothing_found_is_an_empty_list() {
        assert!(parse_songs(page(r#"{"Items": []}"#)).is_empty());
        assert!(parse_songs(page("{}")).is_empty());
    }

    #[test]
    fn only_jellyfin_ids_are_turned_into_jellyfin_requests() {
        assert_eq!(jellyfin_id("jf-0123abcd").unwrap(), "0123abcd");
        for bad in ["0123abcd", "up-1", "jf-", "jf-../x", "jf-a/b"] {
            assert!(jellyfin_id(bad).is_err(), "{bad}");
        }
    }
}
