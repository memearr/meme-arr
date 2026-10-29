//! Disk-usage helpers for the System tab's storage breakdown.

use serde::Serialize;
use std::path::Path;
use std::time::UNIX_EPOCH;

/// Immediate subdirectories of `path` (full paths, sorted), for the setup
/// wizard's "what's mounted here?" picker. Directories only -- file names
/// aren't exposed -- capped so a huge folder can't produce a huge response.
pub fn list_dirs(path: &Path) -> std::io::Result<Vec<String>> {
    const CAP: usize = 500;
    let mut out: Vec<String> = std::fs::read_dir(path)?
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| e.path().to_string_lossy().replace('\\', "/"))
        .collect();
    out.sort_by_key(|p| p.to_lowercase());
    out.truncate(CAP);
    Ok(out)
}

/// Human-readable size in binary units for log messages ("1.50 GB").
pub fn fmt_bytes(n: i64) -> String {
    let n = n.max(0) as f64;
    if n < 1024.0 {
        return format!("{n:.0} B");
    }
    let units = ["KB", "MB", "GB", "TB"];
    let mut v = n / 1024.0;
    let mut i = 0;
    while v >= 1024.0 && i < units.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    format!("{v:.2} {}", units[i])
}

/// Recursively sum the size (bytes) and count of files under `path`.
/// Best-effort: unreadable entries are skipped rather than failing the whole
/// walk, and a missing `path` simply returns zero.
pub fn dir_size(path: &Path) -> (u64, u64) {
    let mut bytes = 0u64;
    let mut files = 0u64;
    walk(path, &mut bytes, &mut files);
    (bytes, files)
}

fn walk(path: &Path, bytes: &mut u64, files: &mut u64) {
    let entries = match std::fs::read_dir(path) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let Ok(meta) = entry.metadata() else { continue };
        if meta.is_dir() {
            walk(&entry.path(), bytes, files);
        } else if meta.is_file() {
            *bytes += meta.len();
            *files += 1;
        }
    }
}

/// Delete every file recursively under `path` (directory structure itself is
/// left in place — harmless empty dirs). Best-effort, same as `dir_size`: a
/// file that can't be removed is skipped rather than aborting the whole clear.
/// Returns the bytes and file count actually freed.
pub fn clear_dir(path: &Path) -> (u64, u64) {
    let mut bytes = 0u64;
    let mut files = 0u64;
    clear_walk(path, &mut bytes, &mut files);
    (bytes, files)
}

fn clear_walk(path: &Path, bytes: &mut u64, files: &mut u64) {
    let entries = match std::fs::read_dir(path) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let Ok(meta) = entry.metadata() else { continue };
        let p = entry.path();
        if meta.is_dir() {
            clear_walk(&p, bytes, files);
        } else if meta.is_file() {
            let len = meta.len();
            if std::fs::remove_file(&p).is_ok() {
                *bytes += len;
                *files += 1;
            }
        }
    }
}

/// One file in a listing (used for the exported-memes browse/re-download
/// view) -- not recursive, since exports land as flat files directly in
/// `OUTPUT_DIR` (see `export.rs`), no subdirectories to walk.
#[derive(Debug, Clone, Serialize)]
pub struct FileEntry {
    pub name: String,
    pub bytes: u64,
    /// Unix seconds. `0` if the filesystem couldn't report an mtime (rare,
    /// but better to show "unknown" than fail the whole listing over it).
    pub modified_at: i64,
}

/// List the files directly inside `path` (not subdirectories), newest
/// first. Best-effort, same spirit as `dir_size`: an unreadable entry is
/// skipped rather than failing the whole listing, and a missing `path`
/// simply returns an empty list.
pub fn list_files(path: &Path) -> Vec<FileEntry> {
    let entries = match std::fs::read_dir(path) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };
    let mut out: Vec<FileEntry> = entries
        .flatten()
        .filter_map(|entry| {
            let meta = entry.metadata().ok()?;
            if !meta.is_file() {
                return None;
            }
            let modified_at = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            Some(FileEntry {
                name: entry.file_name().to_string_lossy().into_owned(),
                bytes: meta.len(),
                modified_at,
            })
        })
        .collect();
    out.sort_by_key(|f| std::cmp::Reverse(f.modified_at));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_dirs_returns_only_directories_sorted_and_errors_on_missing() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("TV")).unwrap();
        std::fs::create_dir(dir.path().join("movies")).unwrap();
        std::fs::write(dir.path().join("secret-file.txt"), "x").unwrap();
        let names: Vec<String> = list_dirs(dir.path())
            .unwrap()
            .iter()
            .map(|p| p.rsplit('/').next().unwrap().to_string())
            .collect();
        assert_eq!(names, ["movies", "TV"]);
        assert!(list_dirs(&dir.path().join("nope")).is_err());
    }

    #[test]
    fn sums_files_recursively() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), b"hello").unwrap(); // 5 bytes
        let sub = dir.path().join("sub");
        std::fs::create_dir(&sub).unwrap();
        std::fs::write(sub.join("b.txt"), b"hi").unwrap(); // 2 bytes

        let (bytes, files) = dir_size(dir.path());
        assert_eq!(bytes, 7);
        assert_eq!(files, 2);
    }

    #[test]
    fn missing_directory_is_zero() {
        let (bytes, files) = dir_size(Path::new("/definitely/not/a/real/path/xyz"));
        assert_eq!(bytes, 0);
        assert_eq!(files, 0);
    }

    #[test]
    fn empty_directory_is_zero() {
        let dir = tempfile::tempdir().unwrap();
        let (bytes, files) = dir_size(dir.path());
        assert_eq!(bytes, 0);
        assert_eq!(files, 0);
    }

    #[test]
    fn clear_dir_removes_files_recursively_and_reports_totals() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), b"hello").unwrap(); // 5 bytes
        let sub = dir.path().join("sub");
        std::fs::create_dir(&sub).unwrap();
        std::fs::write(sub.join("b.txt"), b"hi").unwrap(); // 2 bytes

        let (bytes, files) = clear_dir(dir.path());
        assert_eq!(bytes, 7);
        assert_eq!(files, 2);

        // Files are gone; a second clear finds nothing left to do.
        let (bytes2, files2) = dir_size(dir.path());
        assert_eq!(bytes2, 0);
        assert_eq!(files2, 0);
        let (bytes3, files3) = clear_dir(dir.path());
        assert_eq!(bytes3, 0);
        assert_eq!(files3, 0);
    }

    #[test]
    fn clear_dir_missing_directory_is_a_noop() {
        let (bytes, files) = clear_dir(Path::new("/definitely/not/a/real/path/xyz"));
        assert_eq!(bytes, 0);
        assert_eq!(files, 0);
    }

    #[test]
    fn list_files_returns_flat_files_newest_first() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("older.gif"), b"aaaa").unwrap();
        // Give the second file a distinctly later mtime -- same-millisecond
        // writes on a fast filesystem could otherwise tie.
        std::thread::sleep(std::time::Duration::from_millis(1100));
        std::fs::write(dir.path().join("newer.gif"), b"bb").unwrap();
        // A subdirectory shouldn't appear in a flat listing.
        std::fs::create_dir(dir.path().join("subdir")).unwrap();

        let files = list_files(dir.path());
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].name, "newer.gif");
        assert_eq!(files[0].bytes, 2);
        assert_eq!(files[1].name, "older.gif");
        assert_eq!(files[1].bytes, 4);
        assert!(files[0].modified_at >= files[1].modified_at);
    }

    #[test]
    fn list_files_missing_directory_is_empty() {
        assert!(list_files(Path::new("/definitely/not/a/real/path/xyz")).is_empty());
    }
}
