//! App version + "is there a newer release?" check against GitHub tags.
//!
//! The semver parsing/compare is pure and tested; the network check degrades
//! gracefully (private repos need a `GITHUB_TOKEN`, otherwise the UI just shows
//! the version and a link).

use anyhow::{bail, Result};
use serde::Deserialize;
use std::time::Duration;

/// Parse `v1.2.3` / `1.2` / `2.0.0-rc1` into `(major, minor, patch)`.
pub fn parse_semver(s: &str) -> Option<(u32, u32, u32)> {
    let s = s.trim().trim_start_matches('v');
    let mut it = s.split('.');
    let major = leading_num(it.next()?)?;
    let minor = it.next().and_then(leading_num).unwrap_or(0);
    let patch = it.next().and_then(leading_num).unwrap_or(0);
    Some((major, minor, patch))
}

fn leading_num(s: &str) -> Option<u32> {
    let digits: String = s.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

/// Is `latest` a strictly newer version than `current`?
pub fn is_newer(latest: &str, current: &str) -> bool {
    match (parse_semver(latest), parse_semver(current)) {
        (Some(l), Some(c)) => l > c,
        _ => false,
    }
}

#[derive(Debug, Deserialize)]
struct Tag {
    name: String,
}

/// Fetch the highest semver tag from a GitHub repo. Returns `Ok(None)` if the
/// repo has no semver tags; errors (incl. auth/network failures) bubble up so
/// the caller can report "couldn't check".
pub async fn latest_tag(repo: &str, token: Option<&str>) -> Result<Option<String>> {
    let url = format!("https://api.github.com/repos/{repo}/tags?per_page=100");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()?;
    let mut req = client
        .get(&url)
        .header("User-Agent", "meme-arr")
        .header("Accept", "application/vnd.github+json");
    if let Some(t) = token {
        req = req.header("Authorization", format!("Bearer {t}"));
    }
    let resp = req.send().await?;
    if !resp.status().is_success() {
        bail!("GitHub returned {}", resp.status());
    }
    let tags: Vec<Tag> = resp.json().await?;
    let latest = tags
        .into_iter()
        .filter_map(|t| parse_semver(&t.name).map(|v| (v, t.name)))
        .max_by_key(|(v, _)| *v)
        .map(|(_, name)| name);
    Ok(latest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_versions() {
        assert_eq!(parse_semver("v0.4.0"), Some((0, 4, 0)));
        assert_eq!(parse_semver("1.2"), Some((1, 2, 0)));
        assert_eq!(parse_semver("2.0.0-rc1"), Some((2, 0, 0)));
        assert_eq!(parse_semver("nope"), None);
    }

    #[test]
    fn compares_versions() {
        assert!(is_newer("v0.5.0", "v0.4.0"));
        assert!(is_newer("v1.0.0", "v0.9.9"));
        assert!(is_newer("v0.4.1", "v0.4.0"));
        assert!(!is_newer("v0.4.0", "v0.4.0"));
        assert!(!is_newer("v0.3.0", "v0.4.0"));
        assert!(!is_newer("garbage", "v0.4.0"));
    }
}
