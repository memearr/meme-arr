//! Explains why a requested subtitle hasn't become searchable.
//!
//! The app only indexes a subtitle that is a **file beside the video** (named
//! like it) or embedded in it. When a download is "accepted" but the request
//! stays "Waiting", the cause is almost always where the media server put the
//! file. This module gathers what the app can see locally (the video, the
//! files next to it, what the indexer would pick, the stored fingerprint) and
//! combines it with what the server says it has for the item, then states the
//! most likely reason in plain words.

use crate::config::PathRewrite;
use crate::db;
use crate::index::{find_sidecar, fingerprint};
use crate::mediaserver::{ServerKind, ServerSubtitle};
use crate::paths;
use anyhow::Result;
use serde::Serialize;
use std::path::Path;

/// Extensions that are subtitle files at all (whether or not we can read them).
const SUBTITLE_EXTS: &[&str] = &["srt", "vtt", "ass", "ssa", "sub", "idx", "sup", "smi"];
/// Extensions the indexer can turn into text.
const INDEXABLE_EXTS: &[&str] = &["srt", "vtt", "ass", "ssa"];
/// Extensions that are video files, to tell whose subtitle a file is.
const VIDEO_EXTS: &[&str] = &[
    "mkv", "mp4", "m4v", "avi", "mov", "wmv", "flv", "webm", "ts", "m2ts", "mpg", "mpeg",
];
/// How many folder entries are listed (a season folder can be large).
const MAX_FILES: usize = 40;

#[derive(Debug, Clone, Serialize)]
pub struct FolderFile {
    pub name: String,
    pub size: u64,
    /// Its name starts with the video's name, which the indexer requires.
    pub matches_video: bool,
    /// A text format the indexer can read (not image-based `.sub`/`.sup`).
    pub indexable: bool,
    /// This is the one the indexer would use.
    pub picked: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct LocalReport {
    pub item_known: bool,
    pub item_title: Option<String>,
    /// The video's path as the media server reports it.
    pub server_path: Option<String>,
    /// That path after the path mappings (where this app looks).
    pub mapped_path: Option<String>,
    pub video_reachable: bool,
    pub folder: Option<String>,
    /// Subtitle files in the video's folder that are this video's or that
    /// belong to no video at all. (A season folder holds every episode's
    /// subtitles; those aren't listed, they'd only be noise.)
    pub files: Vec<FolderFile>,
    /// How many subtitle files in the folder belong to *other* videos.
    pub other_videos_subtitles: usize,
    /// The subtitle file the indexer would use, if any.
    pub sidecar: Option<String>,
    pub indexed_lines: i64,
    /// `[mtime, size]` recorded at the last index pass / right now.
    pub stored_fingerprint: Option<[i64; 2]>,
    pub current_fingerprint: Option<[i64; 2]>,
}

/// What the media server itself says about the item's subtitles.
#[derive(Debug, Clone, Serialize)]
pub struct ServerView {
    pub kind: &'static str,
    /// `None` if the server couldn't be asked (see `error`).
    pub streams: Option<Vec<ServerSubtitle>>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    /// `"ok"` | `"info"` | `"warn"` | `"error"`.
    pub level: &'static str,
    pub text: String,
}

fn finding(level: &'static str, text: impl Into<String>) -> Finding {
    Finding {
        level,
        text: text.into(),
    }
}

fn lower_ext(p: &Path) -> String {
    p.extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

/// Everything the app can determine locally about one item's subtitles.
/// Blocking (reads the database and the folder).
pub fn local_report(
    conn: &rusqlite::Connection,
    mappings: &[PathRewrite],
    rating_key: &str,
) -> Result<LocalReport> {
    let Some(item) = db::media_item(conn, rating_key)? else {
        return Ok(LocalReport {
            item_known: false,
            item_title: None,
            server_path: None,
            mapped_path: None,
            video_reachable: false,
            folder: None,
            files: vec![],
            other_videos_subtitles: 0,
            sidecar: None,
            indexed_lines: 0,
            stored_fingerprint: None,
            current_fingerprint: None,
        });
    };
    let server_path = item.file.clone();
    let mapped = server_path.as_deref().map(|f| paths::rewrite(f, mappings));
    let reachable = mapped.as_ref().is_some_and(|p| p.is_file());
    let picked = mapped
        .as_deref()
        .filter(|_| reachable)
        .and_then(find_sidecar);

    let mut files = Vec::new();
    let mut other_videos_subtitles = 0;
    let folder = mapped
        .as_ref()
        .and_then(|p| p.parent().map(Path::to_path_buf));
    if let (Some(video), Some(dir)) = (mapped.as_ref().filter(|_| reachable), folder.as_ref()) {
        let stem_of = |p: &Path| {
            p.file_stem()
                .map(|s| s.to_string_lossy().to_lowercase())
                .unwrap_or_default()
        };
        let our_stem = stem_of(video);
        let entries: Vec<_> = std::fs::read_dir(dir)
            .map(|rd| rd.flatten().collect())
            .unwrap_or_default();
        // The other videos in the folder, so their subtitles aren't mistaken
        // for stray files.
        let other_video_stems: Vec<String> = entries
            .iter()
            .map(|e| e.path())
            .filter(|p| VIDEO_EXTS.contains(&lower_ext(p).as_str()))
            .map(|p| stem_of(&p))
            .filter(|s| *s != our_stem)
            .collect();
        for e in &entries {
            let path = e.path();
            let ext = lower_ext(&path);
            if !SUBTITLE_EXTS.contains(&ext.as_str()) {
                continue;
            }
            let file_stem = stem_of(&path);
            let ours = file_stem.starts_with(&our_stem);
            if !ours
                && other_video_stems
                    .iter()
                    .any(|v| file_stem.starts_with(v.as_str()))
            {
                other_videos_subtitles += 1;
                continue;
            }
            files.push(FolderFile {
                matches_video: ours,
                indexable: INDEXABLE_EXTS.contains(&ext.as_str()),
                picked: picked.as_deref() == Some(path.as_path()),
                size: e.metadata().map(|m| m.len()).unwrap_or(0),
                name: e.file_name().to_string_lossy().to_string(),
            });
        }
        files.sort_by(|a, b| a.name.cmp(&b.name));
        files.truncate(MAX_FILES);
    }

    let stored = db::media_fingerprint(conn, rating_key)?.map(|(m, s, _)| [m, s]);
    let current = mapped
        .as_deref()
        .filter(|_| reachable)
        .and_then(|p| fingerprint(p).ok())
        .map(|(m, s)| [m, s]);

    Ok(LocalReport {
        item_known: true,
        item_title: Some(item.title),
        server_path,
        mapped_path: mapped.map(|p| p.to_string_lossy().replace('\\', "/")),
        video_reachable: reachable,
        folder: folder.map(|p| p.to_string_lossy().replace('\\', "/")),
        files,
        other_videos_subtitles,
        sidecar: picked.map(|p| {
            p.file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default()
        }),
        indexed_lines: db::cue_count_for(conn, rating_key)?,
        stored_fingerprint: stored,
        current_fingerprint: current,
    })
}

fn list(names: impl IntoIterator<Item = String>) -> String {
    names.into_iter().collect::<Vec<_>>().join(", ")
}

/// The most likely explanation, in plain words, most important first.
pub fn findings(local: &LocalReport, server: &ServerView) -> Vec<Finding> {
    let label = if server.kind == ServerKind::Jellyfin.as_str() {
        "Jellyfin"
    } else {
        "Plex"
    };
    let mut out = Vec::new();

    if !local.item_known {
        out.push(finding(
            "error",
            "This item isn't in the index (its library may be unmapped, or it hasn't been scanned yet), so there's nothing to index a subtitle into.",
        ));
        return out;
    }
    if !local.video_reachable {
        out.push(finding(
            "error",
            format!(
                "This app can't find the video file. {label} reports it at {} and the path mappings turn that into {}, which doesn't exist here. Until that mapping is fixed (Settings), no subtitle beside the video can be seen.",
                local.server_path.as_deref().unwrap_or("(unknown)"),
                local.mapped_path.as_deref().unwrap_or("(unknown)")
            ),
        ));
        return out;
    }

    if let Some(sidecar) = &local.sidecar {
        if local.indexed_lines > 0 {
            out.push(finding(
                "ok",
                format!(
                    "Indexed: {} subtitle lines, from the file {sidecar} beside the video.",
                    local.indexed_lines
                ),
            ));
        } else {
            out.push(finding(
                "warn",
                format!(
                    "A subtitle file is beside the video ({sidecar}) but it has produced no indexed lines. Re-index to try again; if it still gives nothing, check the file is readable text and not empty."
                ),
            ));
        }
        if local.stored_fingerprint != local.current_fingerprint {
            out.push(finding(
                "info",
                "The video or its subtitle file has changed since the last index pass, so Re-index will pick up the change.",
            ));
        }
        return out;
    }

    // ---- no usable file beside the video ----
    let ignored: Vec<String> = local
        .files
        .iter()
        .filter(|f| !f.matches_video)
        .map(|f| f.name.clone())
        .collect();
    if !ignored.is_empty() {
        out.push(finding(
            "warn",
            format!(
                "There are subtitle files in the video's folder whose names don't start with the video's name, so the indexer ignores them: {}.",
                list(ignored)
            ),
        ));
    }
    let image_based: Vec<String> = local
        .files
        .iter()
        .filter(|f| f.matches_video && !f.indexable)
        .map(|f| f.name.clone())
        .collect();
    if !image_based.is_empty() {
        out.push(finding(
            "warn",
            format!(
                "Found image-based subtitle files ({}). They can't be searched as text without OCR.",
                list(image_based)
            ),
        ));
    }

    match (&server.streams, &server.error) {
        (Some(streams), _) => {
            let external: Vec<&ServerSubtitle> = streams.iter().filter(|s| s.external).collect();
            if !external.is_empty() {
                let described = list(external.iter().map(|s| {
                    match (&s.path, &s.key) {
                        (Some(p), _) => format!("{} ({p})", s.name),
                        (None, Some(k)) => format!("{} (stream {k})", s.name),
                        _ => s.name.clone(),
                    }
                }));
                if server.kind == ServerKind::Plex.as_str() {
                    // Plex keeps downloaded/uploaded subtitles in its database;
                    // the indexer reads them through the API.
                    if local.indexed_lines > 0 {
                        out.push(finding(
                            "ok",
                            format!(
                                "Indexed: {} subtitle lines. Plex keeps its subtitle ({described}) in its own database rather than as a file, and this app reads it through Plex.",
                                local.indexed_lines
                            ),
                        ));
                    } else {
                        out.push(finding(
                            "info",
                            format!(
                                "Plex lists {} external subtitle(s) for this item ({described}). Plex keeps these in its own database, not as a file beside the video; Re-index reads the text through Plex. If a re-index still finds no lines, the subtitle may be image-based or Plex may have refused to serve it (the Activity log says which).",
                                external.len()
                            ),
                        ));
                    }
                } else {
                    out.push(finding(
                        "warn",
                        format!(
                            "{label} lists {} external subtitle(s) for this item ({described}) but none is a file beside the video. {label} most likely stored the downloaded subtitle in its own storage, which this app can't read. Look for the library option \"Save subtitles with media\" (the wording varies by Jellyfin version), and make sure the server is allowed to write to the media folder.",
                            external.len()
                        ),
                    ));
                }
            } else {
                out.push(finding(
                    "warn",
                    format!(
                        "{label} doesn't list any downloaded subtitle for this item, and none is beside the video. The download may not have completed: check {label}'s own log, whether the subtitle provider's daily download limit was reached, and that the server can write to the media folder."
                    ),
                ));
            }
        }
        (None, Some(e)) => out.push(finding(
            "info",
            format!("Couldn't ask {label} what it has for this item ({e}), so only this app's side is shown."),
        )),
        (None, None) => {}
    }
    if out.is_empty() {
        out.push(finding("info", "No subtitle file is beside the video."));
    }
    out
}

/// When "save fetched subtitles" is on, what the next re-index would write: the
/// file name, beside the video. `None` unless it would actually write one.
pub fn save_plan(local: &LocalReport, server: &ServerView, enabled: bool) -> Option<Finding> {
    if !enabled
        || server.kind != ServerKind::Plex.as_str()
        || local.sidecar.is_some()
        || !local.video_reachable
    {
        return None;
    }
    let stream = crate::remote_sub::pick_stream(server.streams.as_deref()?)?;
    let target =
        crate::remote_sub::counterpart_path(Path::new(local.mapped_path.as_deref()?), stream)?;
    Some(finding(
        "info",
        format!(
            "\"Save fetched subtitles\" is on: Re-index will also save Plex's subtitle as {} next to the video (a file with that name is never overwritten, and a read-only media folder is reported, not fatal).",
            target.file_name()?.to_string_lossy()
        ),
    ))
}

/// One line for the logs: the facts the indexer worked from.
pub fn summary_line(local: &LocalReport) -> String {
    if !local.item_known {
        return "item not in the index".into();
    }
    let files = if local.files.is_empty() {
        "none".to_string()
    } else {
        list(local.files.iter().map(|f| {
            format!(
                "{}{}",
                f.name,
                if f.picked {
                    " [used]"
                } else if !f.matches_video {
                    " [name doesn't match the video]"
                } else if !f.indexable {
                    " [image-based]"
                } else {
                    ""
                }
            )
        }))
    };
    format!(
        "video {} ({}); subtitle files beside it: {}; indexed lines: {}; fingerprint stored {:?} vs now {:?}",
        local.mapped_path.as_deref().unwrap_or("?"),
        if local.video_reachable { "reachable" } else { "NOT reachable" },
        files,
        local.indexed_lines,
        local.stored_fingerprint,
        local.current_fingerprint,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local() -> LocalReport {
        LocalReport {
            item_known: true,
            item_title: Some("Pilot".into()),
            server_path: Some("/data/tv/Show/S01E01.mkv".into()),
            mapped_path: Some("/media/tv/Show/S01E01.mkv".into()),
            video_reachable: true,
            folder: Some("/media/tv/Show".into()),
            files: vec![],
            other_videos_subtitles: 0,
            sidecar: None,
            indexed_lines: 0,
            stored_fingerprint: Some([1, 100]),
            current_fingerprint: Some([1, 100]),
        }
    }
    fn server(kind: &'static str, streams: Option<Vec<ServerSubtitle>>) -> ServerView {
        ServerView {
            kind,
            streams,
            error: None,
        }
    }
    fn stream(name: &str, external: bool, path: Option<&str>) -> ServerSubtitle {
        ServerSubtitle {
            name: name.into(),
            language: Some("English".into()),
            format: Some("srt".into()),
            external,
            path: path.map(String::from),
            key: if external && path.is_none() {
                Some("/library/streams/9".into())
            } else {
                None
            },
            forced: false,
            hearing_impaired: false,
        }
    }
    fn file(name: &str, matches: bool, indexable: bool, picked: bool) -> FolderFile {
        FolderFile {
            name: name.into(),
            size: 10,
            matches_video: matches,
            indexable,
            picked,
        }
    }
    fn levels(f: &[Finding]) -> Vec<&'static str> {
        f.iter().map(|x| x.level).collect()
    }

    #[test]
    fn an_unknown_item_or_unreachable_video_is_the_whole_story() {
        let mut l = local();
        l.item_known = false;
        let f = findings(&l, &server("plex", None));
        assert_eq!((f.len(), f[0].level), (1, "error"));
        assert!(f[0].text.contains("isn't in the index"));

        let mut l = local();
        l.video_reachable = false;
        let f = findings(&l, &server("plex", Some(vec![stream("x", true, None)])));
        assert_eq!((f.len(), f[0].level), (1, "error"));
        assert!(
            f[0].text.contains("/data/tv/Show/S01E01.mkv")
                && f[0].text.contains("/media/tv/Show/S01E01.mkv")
        );
    }

    #[test]
    fn a_sidecar_with_lines_is_a_success_and_one_without_is_a_warning() {
        let mut l = local();
        l.sidecar = Some("S01E01.en.srt".into());
        l.indexed_lines = 812;
        let f = findings(&l, &server("plex", None));
        assert_eq!(levels(&f), ["ok"]);
        assert!(f[0].text.contains("812") && f[0].text.contains("S01E01.en.srt"));

        l.indexed_lines = 0;
        assert_eq!(levels(&findings(&l, &server("plex", None))), ["warn"]);
    }

    #[test]
    fn a_changed_fingerprint_says_re_index_will_pick_it_up() {
        let mut l = local();
        l.sidecar = Some("a.srt".into());
        l.indexed_lines = 5;
        l.current_fingerprint = Some([2, 250]);
        let f = findings(&l, &server("jellyfin", None));
        assert_eq!(levels(&f), ["ok", "info"]);
        assert!(f[1].text.contains("Re-index"));
    }

    #[test]
    fn the_likely_cause_when_the_server_has_it_but_not_beside_the_video() {
        let l = local(); // no sidecar
        let f = findings(
            &l,
            &server(
                "plex",
                Some(vec![
                    stream("English (SRT)", true, None),
                    stream("Embedded", false, None),
                ]),
            ),
        );
        assert_eq!(levels(&f), ["info"]);
        assert!(f[0].text.contains("Plex lists 1 external subtitle"));
        assert!(f[0].text.contains("stream /library/streams/9"));
        assert!(f[0].text.contains("own database") && f[0].text.contains("Re-index reads"));

        // Once those lines have been indexed from Plex's copy it's green.
        let mut done = local();
        done.indexed_lines = 640;
        let f = findings(
            &done,
            &server("plex", Some(vec![stream("English (SRT)", true, None)])),
        );
        assert_eq!(levels(&f), ["ok"]);
        assert!(f[0].text.contains("640") && f[0].text.contains("reads it through Plex"));

        // Jellyfin reports the path it saved to; it is quoted back.
        let f = findings(
            &l,
            &server(
                "jellyfin",
                Some(vec![stream(
                    "English",
                    true,
                    Some("/config/metadata/subs/1.srt"),
                )]),
            ),
        );
        assert!(f[0].text.contains("Jellyfin lists 1 external subtitle"));
        assert!(f[0].text.contains("/config/metadata/subs/1.srt"));
        assert!(f[0].text.contains("Save subtitles with media"));
    }

    #[test]
    fn when_the_server_lists_nothing_the_download_may_not_have_finished() {
        let l = local();
        for streams in [vec![], vec![stream("Embedded", false, None)]] {
            let f = findings(&l, &server("jellyfin", Some(streams)));
            assert_eq!(levels(&f), ["warn"]);
            assert!(f[0].text.contains("doesn't list any downloaded subtitle"));
            assert!(f[0].text.contains("daily download limit"));
        }
    }

    #[test]
    fn the_save_plan_names_the_file_only_when_it_would_be_written() {
        let mut l = local();
        l.mapped_path = Some("/media/Show/S01E01.mkv".into());
        let s = server("plex", Some(vec![stream("English (SRT)", true, None)]));
        assert!(save_plan(&l, &s, false).is_none(), "off by default");
        let f = save_plan(&l, &s, true).unwrap();
        assert!(f.text.contains("S01E01.en.srt") && f.text.contains("never overwritten"));
        // a sidecar already there, or a server that isn't Plex: nothing to write
        let mut with_sidecar = l.clone();
        with_sidecar.sidecar = Some("S01E01.en.srt".into());
        assert!(save_plan(&with_sidecar, &s, true).is_none());
        let j = server("jellyfin", Some(vec![stream("English", true, None)]));
        assert!(save_plan(&l, &j, true).is_none());
    }

    #[test]
    fn files_in_the_folder_the_indexer_ignores_are_called_out() {
        let mut l = local();
        l.files = vec![
            file("Other Name.srt", false, true, false),
            file("S01E01.sub", true, false, false),
        ];
        let f = findings(&l, &server("plex", Some(vec![])));
        let all = f
            .iter()
            .map(|x| x.text.as_str())
            .collect::<Vec<_>>()
            .join(" | ");
        assert!(
            all.contains("names don't start with the video's name")
                && all.contains("Other Name.srt")
        );
        assert!(all.contains("image-based") && all.contains("S01E01.sub"));
    }

    #[test]
    fn an_unreachable_server_is_reported_without_losing_the_local_picture() {
        let l = local();
        let f = findings(
            &l,
            &ServerView {
                kind: "plex",
                streams: None,
                error: Some("timed out".into()),
            },
        );
        assert_eq!(levels(&f), ["info"]);
        assert!(f[0].text.contains("timed out"));
    }

    #[test]
    fn the_log_line_names_the_facts_and_marks_each_file() {
        let mut l = local();
        l.files = vec![
            file("S01E01.en.srt", true, true, true),
            file("elsewhere.srt", false, true, false),
            file("S01E01.sup", true, false, false),
        ];
        let s = summary_line(&l);
        assert!(s.contains("S01E01.en.srt [used]"));
        assert!(s.contains("elsewhere.srt [name doesn't match the video]"));
        assert!(s.contains("S01E01.sup [image-based]"));
        assert!(s.contains("reachable") && s.contains("indexed lines: 0"));
        assert_eq!(
            summary_line(&LocalReport {
                item_known: false,
                ..l
            }),
            "item not in the index"
        );
    }

    #[test]
    fn the_real_folder_scan_finds_and_classifies_files() {
        let dir = tempfile::tempdir().unwrap();
        let video = dir.path().join("Movie (2021).mkv");
        std::fs::write(&video, b"v").unwrap();
        std::fs::write(
            dir.path().join("Movie (2021).en.srt"),
            b"1\n00:00:01,000 --> 00:00:02,000\nhi\n",
        )
        .unwrap();
        std::fs::write(dir.path().join("Unrelated.srt"), b"x").unwrap();
        std::fs::write(dir.path().join("Movie (2021).sup"), b"x").unwrap();
        std::fs::write(dir.path().join("notes.txt"), b"x").unwrap();
        // A second video in the same folder, with its own subtitle: not this
        // video's business, and not a stray file either.
        std::fs::write(dir.path().join("Zed Movie (2020).mkv"), b"v").unwrap();
        std::fs::write(dir.path().join("Zed Movie (2020).en.srt"), b"x").unwrap();
        std::fs::write(dir.path().join("Other Film (1999).mkv"), b"v").unwrap();
        std::fs::write(dir.path().join("Other Film (1999).en.srt"), b"x").unwrap();

        let pool = crate::db::test_pool();
        let conn = pool.get().unwrap();
        crate::db::upsert_media(
            &conn,
            "9",
            "movie",
            "Movie",
            None,
            None,
            None,
            Some(2021),
            "/srv/movies/Movie (2021).mkv",
            None,
            1000,
            "direct",
            Some("Movies"),
            1,
            1,
        )
        .unwrap();
        let rules = [PathRewrite {
            from: "/srv/movies".into(),
            to: dir.path().to_string_lossy().replace('\\', "/"),
        }];
        let r = local_report(&conn, &rules, "9").unwrap();
        assert!(r.item_known && r.video_reachable);
        assert_eq!(r.sidecar.as_deref(), Some("Movie (2021).en.srt"));
        let by = |n: &str| r.files.iter().find(|f| f.name == n).unwrap();
        assert!(by("Movie (2021).en.srt").picked && by("Movie (2021).en.srt").matches_video);
        assert!(!by("Unrelated.srt").matches_video);
        assert!(!by("Movie (2021).sup").indexable);
        assert!(
            !r.files.iter().any(|f| f.name == "notes.txt"),
            "non-subtitle files aren't listed"
        );
        assert!(
            !r.files.iter().any(|f| f.name.starts_with("Other Film")),
            "another video's subtitle isn't listed as a stray file: {:?}",
            r.files.iter().map(|f| &f.name).collect::<Vec<_>>()
        );
        assert_eq!(
            r.other_videos_subtitles, 2,
            "both other videos' subtitles are counted instead"
        );

        // Unknown item.
        assert!(!local_report(&conn, &rules, "nope").unwrap().item_known);
    }
}
