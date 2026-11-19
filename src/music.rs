//! Songs fetched from Navidrome or Jellyfin, kept in `DATA_DIR/music` so an export (and the editor's
//! preview) can read them as ordinary files. The cache is a convenience: anything in it
//! can be fetched again, so it is pruned oldest-first to a size limit.
//!
//! A song is identified by its Navidrome id, which is checked here (letters, digits,
//! `-` and `_`) before it is ever used in a file name. What kind of audio a file is is
//! decided from its first bytes.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// The cache is pruned to about this much before a new song is added.
pub const MAX_CACHE_BYTES: u64 = 1024 * 1024 * 1024;

/// Whether `id` is safe to use as (part of) a file name.
pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// `(extension, content type)` for audio whose first bytes say what it is.
pub fn sniff(head: &[u8]) -> Option<(&'static str, &'static str)> {
    let at = |n: usize, s: &[u8]| head.len() >= n + s.len() && &head[n..n + s.len()] == s;
    if at(0, b"ID3") || (head.len() >= 2 && head[0] == 0xFF && head[1] & 0xE0 == 0xE0) {
        Some(("mp3", "audio/mpeg"))
    } else if at(0, b"fLaC") {
        Some(("flac", "audio/flac"))
    } else if at(0, b"OggS") {
        Some(("ogg", "audio/ogg"))
    } else if at(0, b"RIFF") && at(8, b"WAVE") {
        Some(("wav", "audio/wav"))
    } else if at(4, b"ftyp") {
        Some(("m4a", "audio/mp4"))
    } else if at(0, &[0x1A, 0x45, 0xDF, 0xA3]) {
        Some(("webm", "audio/webm"))
    } else {
        None
    }
}

/// Stream the body of `resp` to `dest` through a side file that is renamed when it is all
/// there, so a failed download leaves nothing behind. Gives up past `max_bytes`; `who` names
/// the server in the messages.
pub async fn save_stream(
    mut resp: reqwest::Response,
    dest: &Path,
    max_bytes: u64,
    who: &str,
) -> anyhow::Result<()> {
    use tokio::io::AsyncWriteExt;
    let tmp = dest.with_extension("part");
    let mut file = tokio::fs::File::create(&tmp).await?;
    let mut total = 0u64;
    let result: anyhow::Result<()> = async {
        while let Some(chunk) = resp.chunk().await? {
            total += chunk.len() as u64;
            if total > max_bytes {
                anyhow::bail!(
                    "that song is bigger than the {} MB allowed",
                    max_bytes / 1024 / 1024
                );
            }
            file.write_all(&chunk).await?;
        }
        file.flush().await?;
        Ok(())
    }
    .await;
    drop(file);
    match result {
        Ok(()) if total > 0 => {
            tokio::fs::rename(&tmp, dest).await?;
            Ok(())
        }
        Ok(()) => {
            let _ = tokio::fs::remove_file(&tmp).await;
            anyhow::bail!("{who} sent an empty file")
        }
        Err(e) => {
            let _ = tokio::fs::remove_file(&tmp).await;
            Err(e)
        }
    }
}

/// The content type for a cached file, from its extension.
pub fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("mp3") => "audio/mpeg",
        Some("flac") => "audio/flac",
        Some("ogg") | Some("oga") | Some("opus") => "audio/ogg",
        Some("aac") => "audio/aac",
        Some("wav") => "audio/wav",
        Some("m4a") => "audio/mp4",
        Some("webm") => "audio/webm",
        _ => "application/octet-stream",
    }
}

#[derive(Debug, Clone)]
pub struct Music {
    dir: PathBuf,
}

impl Music {
    pub fn new(dir: PathBuf) -> Self {
        Music { dir }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The cached file for song `id`, if it is there.
    pub fn path_of(&self, id: &str) -> Option<PathBuf> {
        if !valid_id(id) {
            return None;
        }
        let entries = std::fs::read_dir(&self.dir).ok()?;
        for e in entries.flatten() {
            let p = e.path();
            if p.file_stem().and_then(|s| s.to_str()) == Some(id)
                && p.extension().is_some_and(|x| x != "part")
                && p.is_file()
            {
                return Some(p);
            }
        }
        None
    }

    /// Where to download song `id` to (no extension yet: `finish` names it).
    pub fn download_path(&self, id: &str) -> Option<PathBuf> {
        valid_id(id).then(|| self.dir.join(format!("{id}.dl")))
    }

    /// Check a finished download is audio and give it its real name. The file is removed
    /// if it is not audio.
    pub fn finish(&self, id: &str, downloaded: &Path) -> Result<PathBuf, String> {
        use std::io::Read;
        let mut head = [0u8; 16];
        let n = std::fs::File::open(downloaded)
            .and_then(|mut f| f.read(&mut head))
            .map_err(|e| e.to_string())?;
        let Some((ext, _)) = sniff(&head[..n]) else {
            let _ = std::fs::remove_file(downloaded);
            return Err("what the music server sent isn't an audio file this app can use".into());
        };
        if !valid_id(id) {
            return Err("bad song id".into());
        }
        let dest = self.dir.join(format!("{id}.{ext}"));
        std::fs::rename(downloaded, &dest).map_err(|e| e.to_string())?;
        Ok(dest)
    }

    /// Remove the oldest cached songs until what is left fits in `keep_bytes`.
    pub fn prune(&self, keep_bytes: u64) {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return;
        };
        let mut files: Vec<(SystemTime, u64, PathBuf)> = entries
            .flatten()
            .filter_map(|e| {
                let m = e.metadata().ok()?;
                m.is_file().then(|| {
                    (
                        m.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                        m.len(),
                        e.path(),
                    )
                })
            })
            .collect();
        let mut total: u64 = files.iter().map(|f| f.1).sum();
        files.sort();
        for (_, len, path) in files {
            if total <= keep_bytes {
                break;
            }
            if std::fs::remove_file(&path).is_ok() {
                total = total.saturating_sub(len);
            }
        }
    }
}

#[cfg(test)]
#[path = "music_tests.rs"]
mod tests;
