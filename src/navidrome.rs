//! Navidrome (a self-hosted music server): search its songs and fetch one to use as the
//! music under a clip.
//!
//! Navidrome speaks the Subsonic API. What is used: `ping` (is it there, which version),
//! `search3` (songs only), `stream?format=raw` (the original file, not a transcode) and
//! `getCoverArt` (the album's picture).
//! Authentication is the Subsonic token scheme: `t = md5(password + salt)` with a fresh
//! random `s` on every request, so the password itself never travels in a URL.

use anyhow::{bail, Context, Result};
use md5::{Digest, Md5};
use serde::Serialize;
use serde_json::Value;
use std::path::Path;
use std::time::Duration;

/// The Subsonic API version asked for; every Navidrome speaks at least this.
const API_VERSION: &str = "1.16.1";
/// A song fetched for use under a clip may be at most this big.
pub const MAX_TRACK_BYTES: u64 = 200 * 1024 * 1024;
/// A cover picture may be at most this big.
pub const MAX_COVER_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Track {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration_s: u32,
    /// The id of the song's cover picture (see `Client::cover`), or empty when it has none.
    pub cover_art: String,
}

#[derive(Clone)]
pub struct Client {
    base: String,
    user: String,
    password: String,
    http: reqwest::Client,
}

/// `md5(password + salt)` as lowercase hex: the Subsonic `t` parameter.
pub fn token(password: &str, salt: &str) -> String {
    let mut h = Md5::new();
    h.update(password.as_bytes());
    h.update(salt.as_bytes());
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// A Subsonic response's payload, or its error message. Every answer is wrapped in
/// `subsonic-response` with a `status` of `ok` or `failed`.
pub fn envelope(body: &Value) -> Result<&Value> {
    let r = body
        .get("subsonic-response")
        .context("that doesn't look like a Navidrome (Subsonic) server")?;
    if r.get("status").and_then(Value::as_str) == Some("ok") {
        return Ok(r);
    }
    let msg = r
        .pointer("/error/message")
        .and_then(Value::as_str)
        .unwrap_or("it refused the request");
    bail!("{msg}")
}

/// The songs in a `search3` answer. Songs without an id are dropped; a missing duration
/// is 0.
pub fn parse_search(body: &Value) -> Result<Vec<Track>> {
    let r = envelope(body)?;
    let songs = r
        .pointer("/searchResult3/song")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let text = |s: &Value, k: &str| s.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    Ok(songs
        .iter()
        .filter_map(|s| {
            let id = text(s, "id");
            if id.is_empty() {
                return None;
            }
            Some(Track {
                id,
                title: text(s, "title"),
                artist: text(s, "artist"),
                album: text(s, "album"),
                cover_art: text(s, "coverArt"),
                duration_s: s
                    .get("duration")
                    .and_then(Value::as_u64)
                    .unwrap_or(0)
                    .min(u32::MAX as u64) as u32,
            })
        })
        .collect())
}

impl Client {
    pub fn new(url: &str, user: &str, password: &str) -> Self {
        Client {
            base: url.trim().trim_end_matches('/').to_string(),
            user: user.trim().to_string(),
            password: password.to_string(),
            http: crate::tls::client()
                .timeout(Duration::from_secs(20))
                .build()
                .unwrap_or_default(),
        }
    }

    /// The query every request carries, with the given salt.
    fn auth(&self, salt: &str) -> Vec<(&'static str, String)> {
        vec![
            ("u", self.user.clone()),
            ("t", token(&self.password, salt)),
            ("s", salt.to_string()),
            ("v", API_VERSION.to_string()),
            ("c", "meme-arr".to_string()),
            ("f", "json".to_string()),
        ]
    }

    fn request(&self, endpoint: &str, extra: &[(&str, String)]) -> reqwest::RequestBuilder {
        let salt = uuid::Uuid::new_v4().simple().to_string();
        self.http
            .get(format!("{}/rest/{endpoint}", self.base))
            .query(&self.auth(&salt))
            .query(extra)
    }

    async fn json(&self, endpoint: &str, extra: &[(&str, String)]) -> Result<Value> {
        let resp = self
            .request(endpoint, extra)
            .send()
            .await
            .with_context(|| format!("couldn't reach Navidrome at {}", self.base))?;
        let status = resp.status();
        if !status.is_success() {
            bail!("Navidrome answered {status}");
        }
        resp.json()
            .await
            .context("that doesn't look like a Navidrome (Subsonic) server")
    }

    /// Whether the address, user and password work. Returns the server's version.
    pub async fn ping(&self) -> Result<String> {
        let body = self.json("ping", &[]).await?;
        let r = envelope(&body)?;
        Ok(r.get("serverVersion")
            .or_else(|| r.get("version"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string())
    }

    /// Songs matching `query` (at most `limit`).
    pub async fn search(&self, query: &str, limit: u32) -> Result<Vec<Track>> {
        let body = self
            .json(
                "search3",
                &[
                    ("query", query.to_string()),
                    ("songCount", limit.to_string()),
                    ("songOffset", "0".to_string()),
                    ("artistCount", "0".to_string()),
                    ("albumCount", "0".to_string()),
                ],
            )
            .await?;
        parse_search(&body)
    }

    /// The cover picture `id` (a [`Track::cover_art`]) scaled to about `size` pixels: its
    /// content type and bytes. Only a picture is accepted (a failure comes back as JSON).
    pub async fn cover(&self, id: &str, size: u32) -> Result<(String, Vec<u8>)> {
        let resp = self
            .request(
                "getCoverArt",
                &[("id", id.to_string()), ("size", size.to_string())],
            )
            .send()
            .await
            .with_context(|| format!("couldn't reach Navidrome at {}", self.base))?;
        if !resp.status().is_success() {
            bail!("Navidrome answered {}", resp.status());
        }
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
            bail!("Navidrome sent no picture");
        }
        let bytes = resp.bytes().await.context("the picture didn't arrive")?;
        if bytes.is_empty() || bytes.len() > MAX_COVER_BYTES {
            bail!("the picture is empty or too big");
        }
        Ok((kind, bytes.to_vec()))
    }

    /// Download song `id` (the original file) to `dest`, giving up past `max_bytes`.
    /// Written to a side file and renamed, so a failed download leaves nothing behind.
    pub async fn fetch(&self, id: &str, dest: &Path, max_bytes: u64) -> Result<()> {
        let resp = self
            .request(
                "stream",
                &[("id", id.to_string()), ("format", "raw".into())],
            )
            .timeout(Duration::from_secs(300))
            .send()
            .await
            .with_context(|| format!("couldn't reach Navidrome at {}", self.base))?;
        if !resp.status().is_success() {
            bail!("Navidrome answered {}", resp.status());
        }
        // A failure comes back as a (JSON) answer, not audio.
        let is_json = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|t| t.contains("json") || t.contains("xml") || t.contains("text/"));
        if is_json {
            let text = resp.text().await.unwrap_or_default();
            match serde_json::from_str::<Value>(&text) {
                Ok(v) => {
                    envelope(&v)?;
                    bail!("Navidrome sent no audio")
                }
                Err(_) => bail!("Navidrome sent no audio"),
            }
        }
        crate::music::save_stream(resp, dest, max_bytes, "Navidrome").await
    }
}

#[cfg(test)]
#[path = "navidrome_tests.rs"]
mod tests;
