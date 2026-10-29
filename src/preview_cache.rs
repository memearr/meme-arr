//! Rendering preview clips safely.
//!
//! A preview is an ffmpeg transcode that can take several seconds, written to a
//! cache file named after everything that affects it. Writing straight to that
//! name had three ways to hand the browser a video that "can be clicked but
//! never plays":
//!
//! - **A render that failed, was cancelled (the browser moved on) or was killed
//!   (container restart) left a partial file at the cache name.** Every later
//!   request for the same clip found the file and served the broken video, until
//!   the cache was cleared days later.
//! - **Two requests for the same clip at once** (a slider released twice, Reload
//!   preview, the browser retrying): the second saw the file the first was still
//!   writing and served it half-finished.
//! - Nothing checked a cached file before serving it.
//!
//! So: a render goes to a temporary name and is renamed into place only when it
//! finished and looks like a complete MP4; one request renders while identical
//! ones wait for it; a temporary file never outlives a cancelled request; and a
//! cache hit is checked before it's trusted (a file left behind by an older
//! version, or by a crash, is rendered again).

use anyhow::{bail, Result};
use std::collections::HashMap;
use std::future::Future;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use tracing::warn;

/// Does `path` hold an MP4 whose `moov` atom (the index a player needs) is
/// fully there? An ffmpeg write that stopped early has a `mdat` but no `moov`,
/// so this tells a finished file from a partial one without decoding anything.
pub fn mp4_is_complete(path: &Path) -> bool {
    let Ok(mut f) = std::fs::File::open(path) else {
        return false;
    };
    let Ok(len) = f.metadata().map(|m| m.len()) else {
        return false;
    };
    if len < 32 {
        return false;
    }
    let mut pos: u64 = 0;
    let mut saw_ftyp = false;
    while pos + 8 <= len {
        let mut head = [0u8; 8];
        if f.seek(SeekFrom::Start(pos)).is_err() || f.read_exact(&mut head).is_err() {
            return false;
        }
        let size32 = u32::from_be_bytes([head[0], head[1], head[2], head[3]]) as u64;
        let kind = &head[4..8];
        let size = match size32 {
            0 => len - pos, // "extends to the end of the file"
            1 => {
                let mut big = [0u8; 8];
                if f.read_exact(&mut big).is_err() {
                    return false;
                }
                u64::from_be_bytes(big)
            }
            n => n,
        };
        if size < 8 {
            return false;
        }
        if kind == b"ftyp" {
            saw_ftyp = true;
        }
        if kind == b"moov" {
            return saw_ftyp && pos + size <= len;
        }
        pos = match pos.checked_add(size) {
            Some(p) => p,
            None => return false,
        };
    }
    false // ran out of file without finding a complete moov
}

/// A render's temporary file: removed on drop (a failed or cancelled render)
/// unless it was moved into place with [`PartFile::commit`].
struct PartFile {
    path: PathBuf,
    committed: bool,
}

impl PartFile {
    fn new(final_path: &Path) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let name = final_path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        let path = final_path.with_file_name(format!("{name}.part-{}-{n}.mp4", std::process::id()));
        PartFile {
            path,
            committed: false,
        }
    }

    fn commit(&mut self, final_path: &Path) -> std::io::Result<()> {
        std::fs::rename(&self.path, final_path)?;
        self.committed = true;
        Ok(())
    }
}

impl Drop for PartFile {
    fn drop(&mut self) {
        if !self.committed {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

type Locks = Mutex<HashMap<u64, Arc<tokio::sync::Mutex<()>>>>;

fn locks() -> &'static Locks {
    static LOCKS: OnceLock<Locks> = OnceLock::new();
    LOCKS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Make sure `final_path` holds a complete, rendered preview, calling `render`
/// (with a temporary path to write to) only if it doesn't. Identical `key`s are
/// serialised: the first renders, the others wait and then find the finished
/// file. A failed or cancelled render leaves nothing behind.
pub async fn ensure_rendered<F, Fut>(key: u64, final_path: &Path, render: F) -> Result<()>
where
    F: FnOnce(PathBuf) -> Fut,
    Fut: Future<Output = Result<()>>,
{
    let lock = {
        let mut map = locks().lock().unwrap_or_else(|p| p.into_inner());
        map.entry(key).or_default().clone()
    };
    let result = async {
        let _turn = lock.lock().await;
        if final_path.exists() {
            if mp4_is_complete(final_path) {
                return Ok(());
            }
            warn!(
                "cached preview {} is incomplete (an earlier render was interrupted); rendering it again",
                final_path.display()
            );
            let _ = std::fs::remove_file(final_path);
        }
        let mut part = PartFile::new(final_path);
        render(part.path.clone()).await?;
        if !mp4_is_complete(&part.path) {
            bail!("the preview render finished but didn't produce a complete video");
        }
        part.commit(final_path)?;
        Ok(())
    }
    .await;
    // Forget the lock once nobody else is waiting on it (the map holds one
    // reference, `lock` another).
    let mut map = locks().lock().unwrap_or_else(|p| p.into_inner());
    if Arc::strong_count(&lock) <= 2 {
        map.remove(&key);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::time::Duration;

    fn atom(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut v = ((payload.len() + 8) as u32).to_be_bytes().to_vec();
        v.extend_from_slice(kind);
        v.extend_from_slice(payload);
        v
    }

    /// ftyp + moov + mdat, the way `+faststart` lays a finished file out.
    fn good_mp4() -> Vec<u8> {
        let mut v = atom(b"ftyp", b"isom\0\0\0\0isomiso2");
        v.extend(atom(b"moov", &[0u8; 100]));
        v.extend(atom(b"mdat", &[1u8; 5000]));
        v
    }

    fn write(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, bytes).unwrap();
        p
    }

    #[test]
    fn a_finished_mp4_is_recognised() {
        let d = tempfile::tempdir().unwrap();
        assert!(mp4_is_complete(&write(d.path(), "a.mp4", &good_mp4())));
        // moov after mdat (no faststart) is fine too, as long as it is all there
        let mut v = atom(b"ftyp", b"isom\0\0\0\0isomiso2");
        v.extend(atom(b"mdat", &[1u8; 5000]));
        v.extend(atom(b"moov", &[0u8; 100]));
        assert!(mp4_is_complete(&write(d.path(), "b.mp4", &v)));
    }

    #[test]
    fn partial_empty_and_garbage_files_are_not() {
        let d = tempfile::tempdir().unwrap();
        // what a cut-off ffmpeg write looks like: ftyp then an open-ended mdat
        let mut partial = atom(b"ftyp", b"isom\0\0\0\0isomiso2");
        partial.extend_from_slice(&0u32.to_be_bytes()); // size 0 = to end of file
        partial.extend_from_slice(b"mdat");
        partial.extend(vec![7u8; 4000]);
        assert!(!mp4_is_complete(&write(d.path(), "p.mp4", &partial)));
        assert!(!mp4_is_complete(&write(d.path(), "empty.mp4", b"")));
        assert!(!mp4_is_complete(&write(d.path(), "junk.mp4", &[0xAB; 500])));
        assert!(!mp4_is_complete(&d.path().join("missing.mp4")));
        // a moov that is cut off
        let mut cut = good_mp4();
        cut.truncate(24 + 50);
        assert!(!mp4_is_complete(&write(d.path(), "cut.mp4", &cut)));
        // no ftyp: not an MP4
        let mut noftyp = atom(b"moov", &[0u8; 100]);
        noftyp.extend(atom(b"mdat", &[1u8; 5000]));
        assert!(!mp4_is_complete(&write(d.path(), "noftyp.mp4", &noftyp)));
    }

    #[tokio::test]
    async fn a_render_is_written_to_a_temporary_name_and_moved_into_place() {
        let d = tempfile::tempdir().unwrap();
        let final_path = d.path().join("preview-1.mp4");
        let seen = Arc::new(Mutex::new(None::<PathBuf>));
        let s2 = seen.clone();
        ensure_rendered(1, &final_path, |out| async move {
            *s2.lock().unwrap() = Some(out.clone());
            assert!(
                !final_path_exists_marker(&out),
                "the temporary file isn't the cache name"
            );
            std::fs::write(&out, good_mp4())?;
            Ok(())
        })
        .await
        .unwrap();
        let temp = seen.lock().unwrap().clone().unwrap();
        assert_ne!(temp, final_path);
        assert!(final_path.exists() && !temp.exists());
        assert!(mp4_is_complete(&final_path));
    }

    fn final_path_exists_marker(p: &Path) -> bool {
        p.file_name().unwrap().to_string_lossy() == "preview-1.mp4"
    }

    #[tokio::test]
    async fn a_failed_render_leaves_no_file_to_poison_the_next_request() {
        let d = tempfile::tempdir().unwrap();
        let final_path = d.path().join("preview-2.mp4");
        let r = ensure_rendered(2, &final_path, |out| async move {
            std::fs::write(&out, b"half a video")?; // ffmpeg wrote something, then died
            bail!("ffmpeg failed")
        })
        .await;
        assert!(r.is_err());
        assert!(!final_path.exists());
        assert_eq!(
            std::fs::read_dir(d.path()).unwrap().count(),
            0,
            "no temporary file left"
        );
        // ...and the next request renders it properly
        ensure_rendered(2, &final_path, |out| async move {
            std::fs::write(&out, good_mp4())?;
            Ok(())
        })
        .await
        .unwrap();
        assert!(mp4_is_complete(&final_path));
    }

    #[tokio::test]
    async fn a_render_that_writes_an_unusable_file_is_an_error_not_a_cache_entry() {
        let d = tempfile::tempdir().unwrap();
        let final_path = d.path().join("preview-3.mp4");
        let r = ensure_rendered(3, &final_path, |out| async move {
            std::fs::write(&out, vec![0u8; 5000])?;
            Ok(())
        })
        .await;
        assert!(r.is_err());
        assert!(!final_path.exists());
        assert_eq!(std::fs::read_dir(d.path()).unwrap().count(), 0);
    }

    #[tokio::test]
    async fn a_cancelled_request_cleans_up_its_temporary_file() {
        let d = tempfile::tempdir().unwrap();
        let final_path = d.path().join("preview-4.mp4");
        let fp = final_path.clone();
        let task = tokio::spawn(async move {
            let _ = ensure_rendered(4, &fp, |out| async move {
                std::fs::write(&out, b"partial")?;
                tokio::time::sleep(Duration::from_secs(30)).await; // "ffmpeg" still running
                Ok(())
            })
            .await;
        });
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert_eq!(
            std::fs::read_dir(d.path()).unwrap().count(),
            1,
            "the render is under way"
        );
        task.abort(); // the browser went away
        let _ = task.await;
        assert_eq!(
            std::fs::read_dir(d.path()).unwrap().count(),
            0,
            "nothing left behind"
        );
        assert!(!final_path.exists());
        // and the lock isn't stuck: a later request for the same clip renders
        ensure_rendered(4, &final_path, |out| async move {
            std::fs::write(&out, good_mp4())?;
            Ok(())
        })
        .await
        .unwrap();
        assert!(final_path.exists());
    }

    #[tokio::test]
    async fn identical_requests_at_once_render_once_and_all_get_the_finished_file() {
        let d = tempfile::tempdir().unwrap();
        let final_path = d.path().join("preview-5.mp4");
        let renders = Arc::new(AtomicUsize::new(0));
        let mut tasks = Vec::new();
        for _ in 0..6 {
            let (fp, renders) = (final_path.clone(), renders.clone());
            tasks.push(tokio::spawn(async move {
                ensure_rendered(5, &fp, |out| async move {
                    renders.fetch_add(1, Ordering::SeqCst);
                    std::fs::write(&out, b"partial")?; // visible while it renders
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    std::fs::write(&out, good_mp4())?;
                    Ok(())
                })
                .await
                .unwrap();
                // every caller sees the complete file the moment it returns
                assert!(mp4_is_complete(&fp));
            }));
        }
        for t in tasks {
            t.await.unwrap();
        }
        assert_eq!(renders.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn a_half_written_file_left_by_an_older_version_is_rendered_again() {
        let d = tempfile::tempdir().unwrap();
        let final_path = write(
            d.path(),
            "preview-6.mp4",
            b"ftypjunk-left-by-a-killed-ffmpeg",
        );
        ensure_rendered(6, &final_path, |out| async move {
            std::fs::write(&out, good_mp4())?;
            Ok(())
        })
        .await
        .unwrap();
        assert!(mp4_is_complete(&final_path));
    }

    #[tokio::test]
    async fn a_good_cached_file_is_used_without_rendering() {
        let d = tempfile::tempdir().unwrap();
        let final_path = write(d.path(), "preview-7.mp4", &good_mp4());
        ensure_rendered(7, &final_path, |_| async { bail!("must not render") })
            .await
            .unwrap();
    }
}
