//! Runtime configuration, loaded from environment variables.

use anyhow::Result;
use std::path::PathBuf;

/// A single Plex-path -> container-path rewrite rule.
#[derive(Debug, Clone)]
pub struct PathRewrite {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone)]
pub struct Config {
    /// Optional first-run bootstrap of the Plex URL (env `PLEX_URL`). The app
    /// normally reads Plex settings from the database (Settings page); this only
    /// seeds the DB on first start if present.
    pub plex_url: Option<String>,
    /// Optional first-run bootstrap of the Plex token (env `PLEX_TOKEN`).
    pub plex_token: Option<String>,
    /// Optional first-run bootstrap of path mappings (env `PATH_MAP`).
    pub path_map: Vec<PathRewrite>,
    /// Where finished exports are written (a mounted share).
    pub output_dir: PathBuf,
    /// Where the SQLite db, extracted-subtitle cache and temp files live.
    pub data_dir: PathBuf,
    /// Socket address the HTTP server binds to.
    pub bind_addr: String,
    /// Directory containing the built SPA (static assets).
    pub web_dir: PathBuf,
    /// Directory containing bundled caption fonts.
    pub fonts_dir: PathBuf,
    /// Kick off a full library index on startup.
    pub index_on_start: bool,
    /// `ffmpeg` binary (overridable for tests / non-standard installs).
    pub ffmpeg_bin: String,
    /// `ffprobe` binary.
    pub ffprobe_bin: String,
    /// `gifsicle` binary, used as a post-process lossy recompression pass on
    /// GIF exports (overridable for tests / non-standard installs).
    pub gifsicle_bin: String,
    /// This build's version (set at image-build time from the git tag).
    pub app_version: String,
    /// GitHub repo to check for newer releases, `owner/name`.
    pub github_repo: String,
    /// Optional token for the update check (required if the repo is private).
    pub github_token: Option<String>,
    /// Sub-path to host the app under, e.g. `/meme-arr` (env `URL_BASE`) --
    /// like Sonarr's URL Base. Empty = served at the root. Your reverse proxy
    /// must forward the prefix unchanged (not strip it).
    pub url_base: String,
    /// Recovery/bootstrap for the admin password (env `RESET_ADMIN_PASSWORD`).
    /// Checked once at every startup: if set, it (re)sets the stored admin
    /// password to this value, logs a loud warning, and the operator is
    /// expected to remove it afterward — same shape as editing Sonarr's
    /// `config.xml` and restarting to force new credentials, just via an
    /// env var instead of a file edit.
    pub reset_admin_password: Option<String>,
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

/// Normalize a `URL_BASE` value: `""`/`"/"` -> `""`; otherwise a leading `/`,
/// no trailing `/`. Only URL-safe path characters, no empty or `..`
/// segments -- it's interpolated into redirects and the served HTML.
pub fn normalize_url_base(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim().trim_matches('/');
    if trimmed.is_empty() {
        return Ok(String::new());
    }
    for seg in trimmed.split('/') {
        let ok = !seg.is_empty()
            && seg != "."
            && seg != ".."
            && seg
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '~'));
        if !ok {
            return Err(format!(
                "URL_BASE '{raw}' is invalid: use letters, digits and - _ . ~ in each segment (e.g. /meme-arr)"
            ));
        }
    }
    Ok(format!("/{trimmed}"))
}

fn parse_path_map(raw: &str) -> Vec<PathRewrite> {
    // Format: "plexPrefix1=>containerPrefix1,plexPrefix2=>containerPrefix2"
    raw.split(',')
        .filter_map(|pair| {
            let pair = pair.trim();
            if pair.is_empty() {
                return None;
            }
            let (from, to) = pair.split_once("=>")?;
            Some(PathRewrite {
                from: from.trim().to_string(),
                to: to.trim().to_string(),
            })
        })
        .collect()
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let plex_url = std::env::var("PLEX_URL")
            .ok()
            .map(|u| u.trim_end_matches('/').to_string())
            .filter(|u| !u.is_empty());
        let plex_token = std::env::var("PLEX_TOKEN").ok().filter(|t| !t.is_empty());

        let url_base = normalize_url_base(&env_or("URL_BASE", "")).map_err(anyhow::Error::msg)?;

        Ok(Self {
            url_base,
            plex_url,
            plex_token,
            path_map: parse_path_map(&env_or("PATH_MAP", "")),
            output_dir: PathBuf::from(env_or("OUTPUT_DIR", "/output")),
            data_dir: PathBuf::from(env_or("DATA_DIR", "/data")),
            bind_addr: env_or("BIND_ADDR", "0.0.0.0:8080"),
            web_dir: PathBuf::from(env_or("WEB_DIR", "/app/web")),
            fonts_dir: PathBuf::from(env_or("FONTS_DIR", "/app/fonts")),
            index_on_start: env_or("INDEX_ON_START", "true") != "false",
            ffmpeg_bin: env_or("FFMPEG_BIN", "ffmpeg"),
            ffprobe_bin: env_or("FFPROBE_BIN", "ffprobe"),
            gifsicle_bin: env_or("GIFSICLE_BIN", "gifsicle"),
            app_version: std::env::var("APP_VERSION")
                .ok()
                .filter(|v| !v.is_empty())
                .unwrap_or_else(|| format!("v{}", env!("CARGO_PKG_VERSION"))),
            github_repo: env_or("GITHUB_REPO", "memearr/meme-arr"),
            github_token: std::env::var("GITHUB_TOKEN").ok().filter(|t| !t.is_empty()),
            reset_admin_password: std::env::var("RESET_ADMIN_PASSWORD")
                .ok()
                .filter(|p| !p.is_empty()),
        })
    }

    /// DB file path inside `data_dir`.
    pub fn db_path(&self) -> PathBuf {
        self.data_dir.join("index.sqlite3")
    }

    /// Settings backups (see `backup`).
    pub fn backup_dir(&self) -> PathBuf {
        self.data_dir.join("backups")
    }

    /// Temp working directory inside `data_dir`.
    pub fn tmp_dir(&self) -> PathBuf {
        self.data_dir.join("tmp")
    }

    /// Where the compact dialogue index (see `compactindex`) is persisted
    /// between restarts — a rebuildable cache, not authoritative storage, so
    /// deleting this file just costs one rebuild, never data.
    pub fn compact_index_path(&self) -> PathBuf {
        self.data_dir.join("compact_index.bin")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_multi_rule_path_map() {
        let rules = parse_path_map("/volume1/media=>/media, /mnt/tv=>/tv");
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0].from, "/volume1/media");
        assert_eq!(rules[0].to, "/media");
        assert_eq!(rules[1].from, "/mnt/tv");
        assert_eq!(rules[1].to, "/tv");
    }

    #[test]
    fn url_base_is_normalized_or_rejected() {
        for (raw, want) in [
            ("", ""),
            ("/", ""),
            ("  ", ""),
            ("meme-arr", "/meme-arr"),
            ("/meme-arr/", "/meme-arr"),
            ("/a/b", "/a/b"),
        ] {
            assert_eq!(normalize_url_base(raw).unwrap(), want, "{raw:?}");
        }
        for bad in [
            "/a//b",
            "/../x",
            "/a/./b",
            "/has space",
            "/q?x=1",
            "/<script>",
            "/a\\b",
            "http://x",
        ] {
            assert!(normalize_url_base(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn empty_path_map_is_empty() {
        assert!(parse_path_map("").is_empty());
        assert!(parse_path_map("   ").is_empty());
    }
}
