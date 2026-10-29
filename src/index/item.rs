//! Indexing one item: sidecar, embedded track, or the subtitle the server keeps; and the single-item re-index.

use super::*;

pub(super) enum Outcome {
    Indexed,
    Skipped,
}

pub(super) async fn index_one(
    state: &AppState,
    mappings: &[crate::config::PathRewrite],
    item: &crate::mediaserver::Item,
    remote: bool,
    force: bool,
) -> Result<Outcome> {
    let file = match &item.file {
        Some(f) => f,
        None => return Ok(Outcome::Skipped),
    };
    let access = paths::resolve(file, mappings);

    // Determine fingerprint for direct-access files so we can skip unchanged ones.
    let (mut mtime, mut size, direct_path) = match &access {
        AccessMode::Direct(p) => {
            let (mtime, size) = fingerprint(p)?;
            (mtime, size, Some(p.clone()))
        }
        AccessMode::NotMounted => (0, 0, None),
    };

    // Skip only if the file is unchanged AND it was already processed by the
    // current indexing logic (see INDEXER_VERSION) — otherwise a smarter indexer
    // would never actually re-run on previously-seen files.
    if direct_path.is_some() && !force {
        let pool = state.pool.clone();
        let rk = item.rating_key.clone();
        if let Ok(Some((old_mtime, old_size, old_version))) =
            tokio::task::spawn_blocking(move || {
                let conn = pool.get()?;
                db::media_fingerprint(&conn, &rk)
            })
            .await?
        {
            if old_mtime == mtime && old_size == size && size != 0 && old_version >= INDEXER_VERSION
            {
                return Ok(Outcome::Skipped);
            }
        }
    }

    // Probe + extract subtitles only for direct-access files (remote download is
    // too heavy for a bulk index pass).
    let mut cues: Vec<subtitles::Cue> = Vec::new();
    let mut duration_ms = item.duration_ms;
    if let Some(path) = &direct_path {
        let probe = ffmpeg::probe(&state.config.ffprobe_bin, path).await?;
        if probe.duration_ms > 0 {
            duration_ms = probe.duration_ms;
        }

        // 1) Prefer a sidecar subtitle file next to the video (.srt/.vtt/.ass/.ssa).
        if let Some(sidecar) = find_sidecar(path) {
            match load_sidecar(&state.config.ffmpeg_bin, &sidecar).await {
                Ok(text) => cues = subtitles::parse(&text),
                Err(e) => {
                    warn!("sidecar subtitle load failed for {}: {e:#}", item.title);
                    state.log_event(
                        LogLevel::Warn,
                        format!(
                            "Sidecar subtitle failed to load for '{}': {e:#}",
                            item.title
                        ),
                    );
                }
            }
        }

        // 2) Fall back to an embedded text subtitle track.
        if cues.is_empty() {
            if let Some(track) = ffmpeg::pick_track(&probe.subtitle_tracks) {
                let rel = track.rel_index;
                match ffmpeg::extract_srt(&state.config.ffmpeg_bin, path, rel).await {
                    Ok(srt) => cues = subtitles::parse(&srt),
                    Err(e) => {
                        warn!("subtitle extract failed for {}: {e:#}", item.title);
                        state.log_event(
                            LogLevel::Warn,
                            format!(
                                "Embedded subtitle extraction failed for '{}': {e:#}",
                                item.title
                            ),
                        );
                    }
                }
            }
        }
    }

    // 3) Nothing in or beside the file: Plex keeps subtitles it downloaded (or
    // that were uploaded) in its own database, so ask it for its copy.
    if cues.is_empty() && remote {
        match crate::remote_sub::read_for_item(state, &item.rating_key).await {
            Ok(Some(sub)) if !sub.cues.is_empty() => {
                state.log_event(
                    LogLevel::Info,
                    format!(
                        "Read the subtitle Plex keeps for '{}': {} ({} lines)",
                        item.title,
                        sub.description,
                        sub.cues.len()
                    ),
                );
                cues = sub.cues;
                if state.settings().save_fetched_subtitles {
                    if let Some(video) = &direct_path {
                        if save_counterpart(state, &item.title, video, &sub.stream, &sub.srt) {
                            // The new sidecar is now part of the item's fingerprint;
                            // store that, so the next scan doesn't see a "change".
                            if let Ok((m, s)) = fingerprint(video) {
                                mtime = m;
                                size = s;
                            }
                        }
                    }
                }
            }
            Ok(_) => {}
            Err(e) => {
                warn!(
                    "reading the server's subtitle failed for {}: {e:#}",
                    item.title
                );
                state.log_event(
                    LogLevel::Warn,
                    format!(
                        "Couldn't read the subtitle Plex keeps for '{}': {e:#}",
                        item.title
                    ),
                );
            }
        }
    }

    // Persist (blocking DB work off the async runtime).
    let pool = state.pool.clone();
    let item = item.clone();
    let access_str = access.as_str().to_string();
    let file_path = file.clone();
    tokio::task::spawn_blocking(move || -> Result<()> {
        let mut conn = pool.get()?;
        let media_id = db::upsert_media(
            &conn,
            &item.rating_key,
            &item.kind,
            &item.title,
            item.show.as_deref(),
            item.season,
            item.episode,
            item.year,
            &file_path,
            item.part_key.as_deref(),
            duration_ms,
            &access_str,
            Some(item.library.as_str()),
            mtime,
            size,
        )?;
        db::insert_cues(&mut conn, media_id, &cues)?;
        db::set_indexer_version(&conn, &item.rating_key, INDEXER_VERSION)?;
        Ok(())
    })
    .await??;

    Ok(Outcome::Indexed)
}

/// Save a subtitle read from the server beside the video, as the setting asks.
/// Returns whether a file was written. Failure (typically a read-only media
/// mount) is logged and never stops the item being indexed.
pub(super) fn save_counterpart(
    state: &AppState,
    title: &str,
    video: &Path,
    stream: &crate::mediaserver::ServerSubtitle,
    srt: &str,
) -> bool {
    let Some(target) = crate::remote_sub::counterpart_path(video, stream) else {
        return false;
    };
    let name = target
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    match crate::remote_sub::write_counterpart(&target, srt) {
        Ok(true) => {
            info!(
                "saved the server's subtitle for {title} as {}",
                target.display()
            );
            state.log_event(
                LogLevel::Info,
                format!("Saved the subtitle for '{title}' as {name} next to the video"),
            );
            true
        }
        Ok(false) => {
            state.log_event(
                LogLevel::Info,
                format!("Did not save a subtitle for '{title}': {name} already exists next to the video"),
            );
            false
        }
        Err(e) => {
            warn!("could not save {}: {e}", target.display());
            state.log_event(
                LogLevel::Warn,
                format!(
                    "Couldn't save {name} next to the video for '{title}': {e}. Is the media folder mounted read-only? The subtitle is still indexed."
                ),
            );
            false
        }
    }
}

pub(super) fn outcome_label(o: &Outcome) -> &'static str {
    match o {
        Outcome::Indexed => "indexed",
        Outcome::Skipped => "unchanged",
    }
}

/// One line describing the item's subtitle situation (see `diagnose`); falls
/// back to a note if it can't be worked out, since this is only for the logs.
pub(super) async fn local_summary(state: &AppState, rating_key: &str) -> String {
    let pool = state.pool.clone();
    let mappings = state.mappings();
    let rk = rating_key.to_string();
    match tokio::task::spawn_blocking(move || -> Result<String> {
        let conn = pool.get()?;
        Ok(crate::diagnose::summary_line(
            &crate::diagnose::local_report(&conn, &mappings, &rk)?,
        ))
    })
    .await
    {
        Ok(Ok(s)) => s,
        _ => "(couldn't inspect the folder)".into(),
    }
}

/// What re-indexing one item did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReindexStatus {
    /// Processed; `lines` is how many subtitle lines it has now.
    Indexed,
    /// The video and any sidecar are exactly as last indexed -- nothing new to find.
    Unchanged,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ReindexResult {
    pub status: ReindexStatus,
    pub lines: i64,
}

/// Re-index a single already-known item (e.g. right after a subtitle was
/// downloaded for it), without walking the whole library. Uses the same logic
/// -- and the same change detection, which now includes the sidecar -- as a
/// scan, so "unchanged" honestly means nothing new is beside the file yet.
pub async fn index_item(state: &AppState, rating_key: &str) -> Result<Option<ReindexResult>> {
    index_item_with(state, rating_key, false).await
}

/// Same, for the scheduled re-check of waiting requests: an item that still
/// has nothing is not written to the Activity log again (it would repeat every
/// run); one that gets subtitle lines is.
pub async fn index_item_quietly(
    state: &AppState,
    rating_key: &str,
) -> Result<Option<ReindexResult>> {
    index_item_with(state, rating_key, true).await
}

pub(super) async fn index_item_with(
    state: &AppState,
    rating_key: &str,
    quiet: bool,
) -> Result<Option<ReindexResult>> {
    let pool = state.pool.clone();
    let rk = rating_key.to_string();
    let Some(item) = tokio::task::spawn_blocking(move || {
        let conn = pool.get()?;
        db::media_item(&conn, &rk)
    })
    .await??
    else {
        return Ok(None);
    };

    // An item with no subtitle lines yet is always worth another look (the
    // server may now hold one), even if the video file itself hasn't changed.
    let pool = state.pool.clone();
    let rk = rating_key.to_string();
    let had_lines = tokio::task::spawn_blocking(move || {
        let conn = pool.get()?;
        db::cue_count_for(&conn, &rk)
    })
    .await??;

    let mappings = state.mappings();
    let outcome = index_one(state, &mappings, &item, true, had_lines == 0).await?;

    let pool = state.pool.clone();
    let rk = rating_key.to_string();
    let lines = tokio::task::spawn_blocking(move || {
        let conn = pool.get()?;
        db::cue_count_for(&conn, &rk)
    })
    .await??;

    // What the indexer worked from, in one line. This is what answers "why is
    // my downloaded subtitle not showing up": which video, which subtitle files
    // sit beside it, which was used, and whether anything changed.
    let facts = local_summary(state, rating_key).await;
    tracing::info!(item = %item.title, lines, outcome = ?outcome_label(&outcome), "{facts}");

    let status = match outcome {
        Outcome::Indexed if lines > 0 => {
            state.log_event(
                LogLevel::Info,
                format!("Re-indexed '{}': {lines} subtitle line(s)", item.title),
            );
            // The research compact index is derived from the cues; refresh it
            // the same way a full pass does.
            tokio::spawn(crate::compact_rebuild::rebuild(state.clone()));
            ReindexStatus::Indexed
        }
        // Looked again because it had no lines, and still has none: nothing new.
        Outcome::Indexed if had_lines == 0 => {
            if !quiet {
                state.log_event(
                    LogLevel::Info,
                    format!(
                        "Re-index of '{}': nothing new to index: {facts}",
                        item.title
                    ),
                );
            }
            ReindexStatus::Unchanged
        }
        Outcome::Indexed => {
            state.log_event(
                LogLevel::Warn,
                format!(
                    "Re-indexed '{}' but found no subtitle lines: {facts}",
                    item.title
                ),
            );
            tokio::spawn(crate::compact_rebuild::rebuild(state.clone()));
            ReindexStatus::Indexed
        }
        Outcome::Skipped => {
            if !quiet {
                state.log_event(
                    LogLevel::Info,
                    format!(
                        "Re-index of '{}': nothing new to index: {facts}",
                        item.title
                    ),
                );
            }
            ReindexStatus::Unchanged
        }
    };
    Ok(Some(ReindexResult { status, lines }))
}
