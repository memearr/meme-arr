//! Songs that don't come from Navidrome: ones uploaded to the app, and a folder of music
//! on disk (the library).
//!
//! Song ids say where a song is from: `up-…` for an upload and `lib-…` for a file in the
//! library; any other id is a Navidrome song, as before. Uploads are kept in
//! `DATA_DIR/music-uploads` and never pruned (unlike the Navidrome cache, they can't be
//! fetched again). The library is read in place and never changed: its files are found by
//! walking the folder, and a song's id is a hash of its path inside it, so no path from the
//! browser is ever used.

use crate::music::sniff;
use crate::navidrome::Track;
use md5::{Digest, Md5};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// An uploaded song may be at most this big.
pub const MAX_UPLOAD_BYTES: usize = 100 * 1024 * 1024;
/// The most songs that may be uploaded at once.
pub const MAX_UPLOADS: usize = 200;
/// File extensions the library's songs may have.
pub const AUDIO_EXTS: &[&str] = &[
    "mp3", "flac", "ogg", "oga", "opus", "m4a", "aac", "wav", "webm",
];
/// The library walk gives up after this many files, and goes no deeper than this.
const MAX_SCANNED: usize = 100_000;
const MAX_DEPTH: usize = 8;

pub const UPLOAD_PREFIX: &str = "up-";
pub const LIBRARY_PREFIX: &str = "lib-";

/// Where a song id says it is from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Upload,
    Library,
    Navidrome,
}

impl Source {
    pub fn of(id: &str) -> Source {
        if id.starts_with(UPLOAD_PREFIX) {
            Source::Upload
        } else if id.starts_with(LIBRARY_PREFIX) {
            Source::Library
        } else {
            Source::Navidrome
        }
    }

    /// The word the API uses for it.
    pub fn name(self) -> &'static str {
        match self {
            Source::Upload => "upload",
            Source::Library => "library",
            Source::Navidrome => "navidrome",
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum UploadError {
    Invalid(String),
    TooLarge,
    Limit,
    NotFound,
}

impl std::fmt::Display for UploadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UploadError::Invalid(m) => write!(f, "{m}"),
            UploadError::TooLarge => write!(
                f,
                "that file is bigger than the {} MB allowed",
                MAX_UPLOAD_BYTES / 1024 / 1024
            ),
            UploadError::Limit => write!(
                f,
                "there are already {MAX_UPLOADS} uploaded songs; remove some first"
            ),
            UploadError::NotFound => write!(f, "no such uploaded song"),
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// `md5(data)` as hex, cut to `n` characters.
fn short_hash(data: &[u8], n: usize) -> String {
    let mut h = Md5::new();
    h.update(data);
    let mut s = hex(&h.finalize());
    s.truncate(n);
    s
}

/// What to show for a file name: no extension, no control characters, not too long.
pub fn display_name(file_name: &str) -> String {
    let base = file_name.rsplit(['/', '\\']).next().unwrap_or(file_name);
    let stem = match base.rsplit_once('.') {
        Some((s, ext)) if !s.is_empty() && ext.len() <= 5 => s,
        _ => base,
    };
    let clean: String = stem.chars().filter(|c| !c.is_control()).collect();
    clean.trim().chars().take(120).collect()
}

/// Whether every word of `query` is in `text` (case does not matter). No words matches nothing.
fn matches(text: &str, query: &str) -> bool {
    let text = text.to_lowercase();
    let mut any = false;
    for w in query.split_whitespace() {
        any = true;
        if !text.contains(&w.to_lowercase()) {
            return false;
        }
    }
    any
}

fn track(id: String, title: String, artist: String, album: String) -> Track {
    Track {
        id,
        title,
        artist,
        album,
        duration_s: 0,
        cover_art: String::new(),
    }
}

// ---- uploads -------------------------------------------------------------------

/// The songs uploaded to the app.
#[derive(Debug, Clone)]
pub struct Uploads {
    dir: PathBuf,
}

impl Uploads {
    pub fn new(dir: PathBuf) -> Self {
        Uploads { dir }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The file for upload `id`, if it is there.
    pub fn path_of(&self, id: &str) -> Option<PathBuf> {
        if !id.starts_with(UPLOAD_PREFIX) || !crate::music::valid_id(id) {
            return None;
        }
        for e in std::fs::read_dir(&self.dir).ok()?.flatten() {
            let p = e.path();
            if p.file_stem().and_then(|s| s.to_str()) == Some(id)
                && p.extension()
                    .and_then(|x| x.to_str())
                    .is_some_and(|x| AUDIO_EXTS.contains(&x))
                && p.is_file()
            {
                return Some(p);
            }
        }
        None
    }

    fn name_file(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.name"))
    }

    fn info(&self, id: &str) -> Track {
        let title = std::fs::read_to_string(self.name_file(id))
            .map(|s| s.trim().to_string())
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| id.to_string());
        track(id.to_string(), title, String::new(), String::new())
    }

    /// The uploaded songs, newest first.
    pub fn list(&self) -> Vec<Track> {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return vec![];
        };
        let mut found: Vec<(SystemTime, String)> = entries
            .flatten()
            .filter_map(|e| {
                let p = e.path();
                let id = p.file_stem()?.to_str()?.to_string();
                let ext_ok = p
                    .extension()
                    .and_then(|x| x.to_str())
                    .is_some_and(|x| AUDIO_EXTS.contains(&x));
                (ext_ok && id.starts_with(UPLOAD_PREFIX)).then(|| {
                    let at = e
                        .metadata()
                        .and_then(|m| m.modified())
                        .unwrap_or(SystemTime::UNIX_EPOCH);
                    (at, id)
                })
            })
            .collect();
        found.sort_by(|a, b| b.cmp(a));
        found.into_iter().map(|(_, id)| self.info(&id)).collect()
    }

    /// The uploaded songs whose name has every word of `query`.
    pub fn search(&self, query: &str) -> Vec<Track> {
        self.list()
            .into_iter()
            .filter(|t| matches(&t.title, query))
            .collect()
    }

    /// Keep an uploaded song. The same song uploaded twice is one song.
    pub fn store(&self, name: &str, bytes: &[u8]) -> Result<Track, UploadError> {
        if bytes.is_empty() {
            return Err(UploadError::Invalid("that file is empty".into()));
        }
        if bytes.len() > MAX_UPLOAD_BYTES {
            return Err(UploadError::TooLarge);
        }
        let Some((ext, _)) = sniff(&bytes[..bytes.len().min(16)]) else {
            return Err(UploadError::Invalid(
                "that isn't an audio file this app can use (MP3, FLAC, Ogg, WAV, M4A or WebM)"
                    .into(),
            ));
        };
        let id = format!("{UPLOAD_PREFIX}{}", short_hash(bytes, 12));
        if self.path_of(&id).is_none() && self.list().len() >= MAX_UPLOADS {
            return Err(UploadError::Limit);
        }
        let io = |e: std::io::Error| UploadError::Invalid(e.to_string());
        std::fs::create_dir_all(&self.dir).map_err(io)?;
        let part = self.dir.join(format!("{id}.part"));
        std::fs::write(&part, bytes).map_err(io)?;
        std::fs::rename(&part, self.dir.join(format!("{id}.{ext}"))).map_err(io)?;
        let title = display_name(name);
        let title = if title.is_empty() { id.clone() } else { title };
        std::fs::write(self.name_file(&id), &title).map_err(io)?;
        Ok(track(id, title, String::new(), String::new()))
    }

    pub fn remove(&self, id: &str) -> Result<(), UploadError> {
        let path = self.path_of(id).ok_or(UploadError::NotFound)?;
        let _ = std::fs::remove_file(self.name_file(id));
        std::fs::remove_file(path).map_err(|e| UploadError::Invalid(e.to_string()))
    }
}

// ---- the library folder --------------------------------------------------------

/// A folder of music on disk, searched by file and folder names.
#[derive(Debug, Clone)]
pub struct Library {
    root: PathBuf,
}

/// `lib-` and a hash of the file's path inside the library (with `/` between folders).
pub fn library_id(relative: &str) -> String {
    format!("{LIBRARY_PREFIX}{}", short_hash(relative.as_bytes(), 16))
}

fn is_audio(p: &Path) -> bool {
    p.extension()
        .and_then(|x| x.to_str())
        .is_some_and(|x| AUDIO_EXTS.contains(&x.to_ascii_lowercase().as_str()))
}

impl Library {
    pub fn new(root: PathBuf) -> Self {
        Library { root }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Every song file in the library as `(path inside it, full path)`, with `/` between
    /// folders. Symlinks and hidden entries are skipped, so the walk can't leave the folder
    /// or loop; it stops after `MAX_SCANNED` files.
    fn files(&self) -> Vec<(String, PathBuf)> {
        let mut out = Vec::new();
        let mut stack = vec![(self.root.clone(), String::new(), 0usize)];
        let mut scanned = 0usize;
        while let Some((dir, rel, depth)) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for e in entries.flatten() {
                scanned += 1;
                if scanned > MAX_SCANNED {
                    return out;
                }
                let name = e.file_name().to_string_lossy().into_owned();
                if name.starts_with('.') {
                    continue;
                }
                let Ok(kind) = e.file_type() else { continue };
                let inner = if rel.is_empty() {
                    name.clone()
                } else {
                    format!("{rel}/{name}")
                };
                if kind.is_dir() {
                    if depth < MAX_DEPTH {
                        stack.push((e.path(), inner, depth + 1));
                    }
                } else if kind.is_file() && is_audio(&e.path()) {
                    out.push((inner, e.path()));
                }
            }
        }
        out.sort();
        out
    }

    /// Songs whose path has every word of `query`, at most `limit`.
    pub fn search(&self, query: &str, limit: usize) -> Vec<Track> {
        self.files()
            .into_iter()
            .filter(|(rel, _)| matches(rel, query))
            .take(limit)
            .map(|(rel, _)| {
                let parts: Vec<&str> = rel.split('/').collect();
                let n = parts.len();
                let album = if n >= 2 { parts[n - 2] } else { "" };
                let artist = if n >= 3 { parts[n - 3] } else { "" };
                track(
                    library_id(&rel),
                    display_name(parts[n - 1]),
                    artist.to_string(),
                    album.to_string(),
                )
            })
            .collect()
    }

    /// The file for library song `id`, if it is still there.
    pub fn path_of(&self, id: &str) -> Option<PathBuf> {
        if !id.starts_with(LIBRARY_PREFIX) || !crate::music::valid_id(id) {
            return None;
        }
        self.files()
            .into_iter()
            .find(|(rel, _)| library_id(rel) == id)
            .map(|(_, path)| path)
    }
}

#[cfg(test)]
#[path = "local_music_tests.rs"]
mod tests;
