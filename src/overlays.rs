//! Pictures and clips the admin uploads to lay over an export (stickers, reaction
//! clips, a logo): kept in `DATA_DIR/overlays`, so they survive upgrades.
//!
//! An overlay is identified by its file name, which is made here (a tidy stem, a short
//! random tail, and the extension of what the bytes really are). What a file is is
//! decided from its first bytes, never from the name it was uploaded under.

use serde::Serialize;
use std::path::{Path, PathBuf};

/// Largest overlay file accepted.
pub const MAX_OVERLAY_BYTES: usize = 25 * 1024 * 1024;
/// Most overlays kept at once.
pub const MAX_OVERLAYS: usize = 100;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OverlayInfo {
    /// The stored file name; what a layer refers to it by.
    pub id: String,
    /// What it was called when uploaded, tidied.
    pub name: String,
    /// `"still"` for a picture that does not move (PNG, JPEG), `"motion"` for one that
    /// does (GIF, WebP, MP4, WebM): a motion overlay loops while it is on screen.
    pub kind: &'static str,
    pub bytes: u64,
}

#[derive(Debug, PartialEq, Eq)]
pub enum OverlayError {
    Invalid(String),
    TooLarge,
    Limit,
    NotFound,
}

impl std::fmt::Display for OverlayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OverlayError::Invalid(why) => write!(f, "{why}"),
            OverlayError::TooLarge => write!(
                f,
                "that file is larger than {} MB",
                MAX_OVERLAY_BYTES / 1024 / 1024
            ),
            OverlayError::Limit => write!(
                f,
                "at most {MAX_OVERLAYS} overlays can be kept; delete one first"
            ),
            OverlayError::NotFound => write!(f, "no such overlay"),
        }
    }
}

/// What the first bytes say a file is: its extension, and whether it moves.
pub fn sniff(bytes: &[u8]) -> Option<(&'static str, bool)> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        Some(("png", false))
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some(("jpg", false))
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some(("gif", true))
    } else if bytes.len() > 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some(("webp", true))
    } else if bytes.len() > 12 && &bytes[4..8] == b"ftyp" {
        Some(("mp4", true))
    } else if bytes.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) {
        Some(("webm", true))
    } else {
        None
    }
}

fn kind_of_ext(ext: &str) -> Option<&'static str> {
    match ext {
        "png" | "jpg" => Some("still"),
        "gif" | "webp" | "mp4" | "webm" => Some("motion"),
        _ => None,
    }
}

/// The content type to serve a stored overlay as.
pub fn content_type(id: &str) -> &'static str {
    match id.rsplit('.').next().unwrap_or("") {
        "png" => "image/png",
        "jpg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        _ => "application/octet-stream",
    }
}

/// A file name's stem, kept to letters, digits, space, `-` and `_`.
fn tidy_stem(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let stem = base.rsplit_once('.').map_or(base, |(s, _)| s);
    let cleaned: String = stem
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, ' ' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let cleaned: String = cleaned.trim().chars().take(40).collect();
    if cleaned.is_empty() {
        "overlay".into()
    } else {
        cleaned
    }
}

/// The folder of uploaded overlays.
#[derive(Debug, Clone)]
pub struct Overlays {
    dir: PathBuf,
}

impl Overlays {
    pub fn new(dir: PathBuf) -> Self {
        Overlays { dir }
    }

    /// Whether `id` can only be a file directly inside the folder: no separators, no
    /// leading dot, and an extension we write.
    fn id_ok(id: &str) -> bool {
        !id.is_empty()
            && id.len() <= 80
            && !id.starts_with('.')
            && !id.contains(['/', '\\', '\0'])
            && id
                .rsplit_once('.')
                .and_then(|(_, e)| kind_of_ext(e))
                .is_some()
    }

    fn info(&self, id: &str, bytes: u64) -> Option<OverlayInfo> {
        let ext = id.rsplit_once('.')?.1;
        let stem = id.rsplit_once('.')?.0;
        // the random tail is the last "-xxxxxxxx"
        let name = stem.rsplit_once('-').map_or(stem, |(n, _)| n).to_string();
        Some(OverlayInfo {
            id: id.to_string(),
            name,
            kind: kind_of_ext(ext)?,
            bytes,
        })
    }

    /// Every overlay, newest first.
    pub fn list(&self) -> Vec<OverlayInfo> {
        let Ok(rd) = std::fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        let mut found: Vec<(std::time::SystemTime, OverlayInfo)> = rd
            .flatten()
            .filter_map(|e| {
                let id = e.file_name().to_string_lossy().to_string();
                if !Self::id_ok(&id) {
                    return None;
                }
                let meta = e.metadata().ok()?;
                Some((meta.modified().ok()?, self.info(&id, meta.len())?))
            })
            .collect();
        found.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.id.cmp(&b.1.id)));
        found.into_iter().map(|(_, i)| i).collect()
    }

    /// Keep an uploaded file after checking it.
    pub fn store(&self, name: &str, bytes: &[u8]) -> Result<OverlayInfo, OverlayError> {
        if bytes.is_empty() {
            return Err(OverlayError::Invalid("that file is empty".into()));
        }
        if bytes.len() > MAX_OVERLAY_BYTES {
            return Err(OverlayError::TooLarge);
        }
        let Some((ext, _)) = sniff(bytes) else {
            return Err(OverlayError::Invalid(
                "only PNG, JPEG, GIF, WebP, MP4 and WebM files can be used".into(),
            ));
        };
        if self.list().len() >= MAX_OVERLAYS {
            return Err(OverlayError::Limit);
        }
        std::fs::create_dir_all(&self.dir)
            .map_err(|e| OverlayError::Invalid(format!("could not keep the file: {e}")))?;
        let tail = uuid::Uuid::new_v4().simple().to_string();
        let id = format!("{}-{}.{ext}", tidy_stem(name), &tail[..8]);
        std::fs::write(self.dir.join(&id), bytes)
            .map_err(|e| OverlayError::Invalid(format!("could not keep the file: {e}")))?;
        self.info(&id, bytes.len() as u64)
            .ok_or_else(|| OverlayError::Invalid("could not keep the file".into()))
    }

    /// The path of a stored overlay, if there is one by that id.
    pub fn path_of(&self, id: &str) -> Option<PathBuf> {
        if !Self::id_ok(id) {
            return None;
        }
        let p = self.dir.join(id);
        p.is_file().then_some(p)
    }

    /// Whether it is a picture that does not move.
    pub fn is_still(id: &str) -> bool {
        id.rsplit_once('.')
            .is_some_and(|(_, e)| kind_of_ext(e) == Some("still"))
    }

    pub fn remove(&self, id: &str) -> Result<(), OverlayError> {
        let p = self.path_of(id).ok_or(OverlayError::NotFound)?;
        std::fs::remove_file(p).map_err(|_| OverlayError::NotFound)
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 0];

    fn store() -> (tempfile::TempDir, Overlays) {
        let dir = tempfile::tempdir().unwrap();
        let o = Overlays::new(dir.path().join("overlays"));
        (dir, o)
    }

    #[test]
    fn what_a_file_is_comes_from_its_bytes() {
        assert_eq!(sniff(PNG), Some(("png", false)));
        assert_eq!(sniff(&[0xFF, 0xD8, 0xFF, 0xE0, 1]), Some(("jpg", false)));
        assert_eq!(sniff(b"GIF89a.."), Some(("gif", true)));
        assert_eq!(sniff(b"RIFF\0\0\0\0WEBPVP8 "), Some(("webp", true)));
        assert_eq!(sniff(b"\0\0\0\x18ftypmp42\0\0"), Some(("mp4", true)));
        assert_eq!(sniff(&[0x1A, 0x45, 0xDF, 0xA3, 0, 0]), Some(("webm", true)));
        assert_eq!(sniff(b"#!/bin/sh\nrm -rf /"), None);
        assert_eq!(sniff(b""), None);
    }

    #[test]
    fn an_upload_is_kept_under_a_tidy_name_with_the_real_extension() {
        let (_d, o) = store();
        // the name says .exe; the bytes say PNG
        let info = o.store("../../my sticker!.exe", PNG).unwrap();
        assert!(info.id.ends_with(".png"), "{}", info.id);
        assert!(info.id.starts_with("my sticker_-"), "{}", info.id);
        assert_eq!(info.name, "my sticker_");
        assert_eq!(info.kind, "still");
        assert!(o.path_of(&info.id).unwrap().is_file());
        assert_eq!(o.list(), vec![info]);
    }

    #[test]
    fn bad_uploads_are_refused() {
        let (_d, o) = store();
        assert!(matches!(
            o.store("a.png", b""),
            Err(OverlayError::Invalid(_))
        ));
        assert!(matches!(
            o.store("a.png", b"not a picture"),
            Err(OverlayError::Invalid(_))
        ));
        let mut big = PNG.to_vec();
        big.resize(MAX_OVERLAY_BYTES + 1, 0);
        assert_eq!(o.store("a.png", &big), Err(OverlayError::TooLarge));
        assert!(o.list().is_empty());
    }

    #[test]
    fn only_a_file_directly_in_the_folder_can_be_reached() {
        let (_d, o) = store();
        let info = o.store("a.png", PNG).unwrap();
        for bad in [
            "../x.png",
            "a/b.png",
            "..\\x.png",
            ".hidden.png",
            "",
            "x.exe",
            "x",
        ] {
            assert!(o.path_of(bad).is_none(), "{bad:?}");
            assert_eq!(o.remove(bad), Err(OverlayError::NotFound), "{bad:?}");
        }
        assert!(o.path_of(&info.id).is_some());
    }

    #[test]
    fn an_overlay_can_be_removed() {
        let (_d, o) = store();
        let info = o.store("a.png", PNG).unwrap();
        o.remove(&info.id).unwrap();
        assert!(o.list().is_empty());
        assert_eq!(o.remove(&info.id), Err(OverlayError::NotFound));
    }

    #[test]
    fn the_number_kept_is_limited() {
        let (_d, o) = store();
        for i in 0..MAX_OVERLAYS {
            o.store(&format!("s{i}.png"), PNG).unwrap();
        }
        assert_eq!(o.store("one more.png", PNG), Err(OverlayError::Limit));
    }

    #[test]
    fn stills_and_motion_are_told_apart() {
        assert!(Overlays::is_still("a-1234abcd.png"));
        assert!(Overlays::is_still("a-1234abcd.jpg"));
        assert!(!Overlays::is_still("a-1234abcd.gif"));
        assert!(!Overlays::is_still("a-1234abcd.mp4"));
        assert_eq!(content_type("x.webm"), "video/webm");
        assert_eq!(content_type("x.png"), "image/png");
    }
}
