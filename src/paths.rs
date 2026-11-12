//! Translate the file paths the media server (Plex or Jellyfin) reports into
//! paths reachable from this container, and decide whether a part is reachable directly on disk.

use crate::config::PathRewrite;
use std::path::{Path, PathBuf};

/// How we can reach a media part's bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccessMode {
    /// File exists on a mounted volume at this (already-rewritten) path.
    Direct(PathBuf),
    /// Not reachable on a mounted volume (no mapping covers it, or the mapped
    /// path doesn't exist here). Such items are listed as unreachable.
    NotMounted,
}

impl AccessMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            AccessMode::Direct(_) => "direct",
            // The stored value predates Jellyfin support and the rename of this
            // variant; it lives in existing databases' `media.access_mode`, so
            // it must not change. Everything that isn't "direct" means "not
            // mounted".
            AccessMode::NotMounted => "plex-download",
        }
    }
}

/// The regular file called `name` directly inside `dir`, or `None`.
///
/// `name` must be a single plain file name, and the resolved file (symlinks
/// followed) must still be inside `dir`, so a link in the folder can't be used
/// to read something elsewhere.
pub fn contained_file(dir: &Path, name: &str) -> Option<PathBuf> {
    let mut parts = Path::new(name).components();
    match (parts.next(), parts.next()) {
        (Some(std::path::Component::Normal(_)), None) => {}
        _ => return None,
    }
    let base = dir.canonicalize().ok()?;
    let file = base.join(name).canonicalize().ok()?;
    (file.starts_with(&base) && file.is_file()).then_some(file)
}

/// `path` resolved (symlinks and `..` followed) and required to sit inside
/// `base`; `None` otherwise. The last part of `path` need not exist yet, but the
/// folder holding it must. Use this on a path before touching the disk with it.
pub fn confine(base: &Path, path: &Path) -> Option<PathBuf> {
    let base = base.canonicalize().ok()?;
    let resolved = match path.canonicalize() {
        Ok(p) => p,
        // Not there yet: resolve the folder it will be created in.
        Err(_) => path.parent()?.canonicalize().ok()?.join(path.file_name()?),
    };
    resolved.starts_with(&base).then_some(resolved)
}

/// Like `confine`, but the path may sit inside any one of `roots`.
pub fn confine_any(roots: &[PathBuf], path: &Path) -> Option<PathBuf> {
    roots.iter().find_map(|r| confine(r, path))
}

/// Folders from a comma- or semicolon-separated list (`BROWSE_ROOTS`).
pub fn parse_roots(list: &str) -> Vec<PathBuf> {
    list.split([',', ';'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .collect()
}

/// Apply the first matching prefix rewrite to a server-reported path. If no rule
/// matches, the original path is returned unchanged (it may still exist if the
/// server and this container happen to share the same mount layout).
///
/// Plex on Windows reports paths with backslashes and case-insensitive drive
/// letters (e.g. `D:\Movies\x.mkv`), while this container is Linux. We normalise
/// separators to `/` and match the prefix case-insensitively so a mapping like
/// `D:\` -> `/media/d` turns `D:\Movies\x.mkv` into `/media/d/Movies/x.mkv`.
pub fn rewrite(server_path: &str, rules: &[PathRewrite]) -> PathBuf {
    let norm = server_path.replace('\\', "/");
    let norm_lower = norm.to_lowercase();
    for rule in rules {
        let from = rule.from.replace('\\', "/");
        let from_lower = from.to_lowercase();
        if norm_lower.starts_with(&from_lower) {
            // Slice the original (separator-normalised) string by the matched
            // prefix length so the remainder keeps its real casing.
            let rest = norm[from.len()..].trim_start_matches('/');
            return Path::new(&rule.to).join(rest);
        }
    }
    PathBuf::from(norm)
}

/// Resolve how to reach a part, checking the filesystem for existence.
pub fn resolve(server_path: &str, rules: &[PathRewrite]) -> AccessMode {
    let mapped = rewrite(server_path, rules);
    if mapped.is_file() {
        AccessMode::Direct(mapped)
    } else {
        AccessMode::NotMounted
    }
}

fn normalize(p: &str) -> String {
    p.replace('\\', "/").to_lowercase()
}

/// Whether any configured mapping rule overlaps this Plex-reported library
/// location — either the rule's prefix covers the location, or the location
/// covers (is an ancestor of) the rule's prefix. Used to decide whether a whole
/// library has been mapped at all, before attempting to index any of its items.
pub fn has_mapping_for(location: &str, rules: &[PathRewrite]) -> bool {
    let loc = normalize(location);
    rules.iter().any(|r| {
        let from = normalize(&r.from);
        loc.starts_with(&from) || from.starts_with(&loc)
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn confine_keeps_paths_inside_the_base() {
        let dir = std::env::temp_dir().join(format!("memearr-confine-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("in")).unwrap();
        let base = dir.join("in");
        assert!(confine(&base, &base).is_some());
        assert!(
            confine(&base, &base.join("new.mp4")).is_some(),
            "a file not made yet"
        );
        assert!(confine(&base, &base.join("..")).is_none());
        assert!(confine(&base, &base.join("..").join("in").join("x")).is_some());
        assert!(confine(&base, &dir).is_none());
        assert!(confine(&base, &dir.join("other.mp4")).is_none());
        assert!(confine(&base, &base.join("no-such-folder").join("f")).is_none());
        let roots = [dir.join("nowhere"), base.clone()];
        assert!(confine_any(&roots, &base.join("a")).is_some());
        assert!(confine_any(&roots, &dir.join("a")).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn roots_are_split_on_commas_and_semicolons() {
        assert_eq!(
            parse_roots("/media, /mnt;/data"),
            ["/media", "/mnt", "/data"].map(PathBuf::from)
        );
        assert!(parse_roots(" , ").is_empty());
    }

    #[test]
    fn contained_file_only_finds_plain_files_inside_the_folder() {
        let dir = std::env::temp_dir().join(format!("memearr-contained-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("a.mp4"), b"x").unwrap();
        std::fs::write(dir.join("sub").join("b.mp4"), b"x").unwrap();
        assert!(contained_file(&dir, "a.mp4").is_some());
        assert!(contained_file(&dir, "missing.mp4").is_none());
        assert!(
            contained_file(&dir, "sub").is_none(),
            "a folder is not a file"
        );
        assert!(contained_file(&dir, "sub/b.mp4").is_none());
        assert!(contained_file(&dir, "../a.mp4").is_none());
        assert!(contained_file(&dir, "/etc/passwd").is_none());
        assert!(contained_file(&dir, "").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    use super::*;

    fn rules() -> Vec<PathRewrite> {
        vec![
            PathRewrite {
                from: "/volume1/media".into(),
                to: "/media".into(),
            },
            PathRewrite {
                from: "/mnt/tv".into(),
                to: "/tv".into(),
            },
        ]
    }

    #[test]
    fn rewrites_first_matching_prefix() {
        let p = rewrite("/volume1/media/Movies/Dune (2021)/Dune.mkv", &rules());
        assert_eq!(p, PathBuf::from("/media/Movies/Dune (2021)/Dune.mkv"));
    }

    #[test]
    fn second_rule_matches() {
        let p = rewrite("/mnt/tv/Show/S01E01.mkv", &rules());
        assert_eq!(p, PathBuf::from("/tv/Show/S01E01.mkv"));
    }

    #[test]
    fn unmatched_path_is_returned_unchanged() {
        let p = rewrite("/somewhere/else/x.mkv", &rules());
        assert_eq!(p, PathBuf::from("/somewhere/else/x.mkv"));
    }

    #[test]
    fn no_rules_passthrough() {
        let p = rewrite("/volume1/media/x.mkv", &[]);
        assert_eq!(p, PathBuf::from("/volume1/media/x.mkv"));
    }

    #[test]
    fn rewrites_windows_backslash_path() {
        let rules = vec![PathRewrite {
            from: "D:\\".into(),
            to: "/media/d".into(),
        }];
        let p = rewrite("D:\\Movies\\Dune (2021)\\Dune.mkv", &rules);
        assert_eq!(p, PathBuf::from("/media/d/Movies/Dune (2021)/Dune.mkv"));
    }

    #[test]
    fn windows_drive_match_is_case_insensitive() {
        let rules = vec![PathRewrite {
            from: "D:\\".into(),
            to: "/media/d".into(),
        }];
        let p = rewrite("d:\\TV\\Show\\S01E01.mkv", &rules);
        assert_eq!(p, PathBuf::from("/media/d/TV/Show/S01E01.mkv"));
    }

    #[test]
    fn windows_mapping_without_trailing_separator() {
        let rules = vec![PathRewrite {
            from: "E:".into(),
            to: "/media/e".into(),
        }];
        let p = rewrite("E:\\Clips\\x.mkv", &rules);
        assert_eq!(p, PathBuf::from("/media/e/Clips/x.mkv"));
    }

    #[test]
    fn missing_file_falls_back_to_download() {
        let mode = resolve("/volume1/media/definitely-not-here.mkv", &rules());
        assert_eq!(mode, AccessMode::NotMounted);
    }

    #[test]
    fn has_mapping_for_matches_when_rule_prefix_covers_location() {
        assert!(has_mapping_for("/volume1/media/Movies", &rules()));
        assert!(has_mapping_for("/MNT/TV/Show", &rules())); // case-insensitive
    }

    #[test]
    fn has_mapping_for_matches_when_location_is_broader_than_rule() {
        let rules = vec![PathRewrite {
            from: "D:\\Movies".into(),
            to: "/media/d/movies".into(),
        }];
        // Plex reports the library root as D:\, which is broader than the mapped
        // D:\Movies prefix — still counts as mapped.
        assert!(has_mapping_for("D:\\", &rules));
    }

    #[test]
    fn has_mapping_for_false_when_nothing_overlaps() {
        assert!(!has_mapping_for("/totally/unrelated", &rules()));
        assert!(!has_mapping_for("E:\\Anime", &rules()));
    }

    #[test]
    fn has_mapping_for_false_with_no_rules() {
        assert!(!has_mapping_for("/anything", &[]));
    }
}
