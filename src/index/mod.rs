//! Background library indexer: walk every movie/episode, resolve its file,
//! extract the best subtitle track with ffmpeg, parse the cues and store them
//! in SQLite/FTS5 for dialogue search.

use crate::paths::{self, AccessMode};
use crate::state::{AppState, LogLevel};
use crate::sync::MutexExt;
use crate::{db, ffmpeg, subtitles};
use anyhow::Result;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{info, warn};

mod item;
mod sidecar;
#[cfg(test)]
mod tests;

pub use self::item::*;
pub(crate) use self::sidecar::*;

/// Bump this whenever the indexing *logic* changes in a way that could produce
/// different results for a file whose bytes haven't changed (new subtitle
/// source, new metadata extracted, etc.). An item is only skipped as "unchanged"
/// when its file fingerprint AND this version both match what's stored — so
/// shipping a smarter indexer automatically re-processes everything once,
/// instead of old rows staying stuck with stale results forever.
///
/// History: 1 = original (embedded tracks only); 2 = sidecar subtitle lookup +
/// per-item library tagging (so pre-sidecar / pre-library rows get redone).
const INDEXER_VERSION: i64 = 2;

/// Run a full index pass. Safe to call again; unchanged files are skipped.
pub async fn run(state: AppState) {
    {
        let mut st = state.index.locked();
        if st.running {
            warn!("index already running; ignoring request");
            return;
        }
        *st = crate::state::IndexStatus {
            running: true,
            ..Default::default()
        };
    }

    if !state.settings().configured() {
        warn!("index requested but Plex is not configured; skipping");
        state.log_event(
            LogLevel::Warn,
            "Reindex requested but Plex is not configured",
        );
        let mut st = state.index.locked();
        st.running = false;
        st.last_error = Some("Plex is not configured (set it in Settings)".into());
        return;
    }

    state.log_event(LogLevel::Info, "Indexing started");
    let result = run_inner(&state).await;

    match &result {
        Ok(()) => {}
        Err(e) => {
            warn!("indexing failed: {e:#}");
            state.log_event(LogLevel::Error, format!("Indexing failed: {e:#}"));
            let mut st = state.index.locked();
            st.last_error = Some(format!("{e:#}"));
        }
    }

    let done = {
        let mut st = state.index.locked();
        st.running = false;
        st.current = None;
        st.finished_at = Some(now_secs());
        info!(
            "index finished: {} indexed, {} skipped, {} errors",
            st.done, st.skipped, st.errors
        );
        if result.is_ok() {
            state.log_event(
                LogLevel::Info,
                format!(
                    "Indexing finished: {} indexed, {} skipped, {} errors",
                    st.done, st.skipped, st.errors
                ),
            );
        }
        st.done
    };

    // Cheap, bounded housekeeping whenever this pass actually wrote anything —
    // reindexing replaces a changed item's cues (delete+reinsert), which
    // leaves free pages behind; fold those back in before they accumulate.
    // Skipped on a no-op pass (nothing to reclaim) and on failure.
    if result.is_ok() && done > 0 {
        let pool = state.pool.clone();
        let checkpointed = tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
            let conn = pool.get()?;
            db::checkpoint_and_trim(&conn)
        })
        .await;
        match checkpointed {
            Ok(Ok(())) => {}
            Ok(Err(e)) => warn!("post-index checkpoint/vacuum failed: {e:#}"),
            Err(e) => warn!("post-index checkpoint task panicked: {e:#}"),
        }
    }
    // subtitle_cue only ever changes here (inserts, delete-then-reinsert on
    // reprocessing, library pruning) -- the compact dialogue index has no
    // in-place update, so any pass that wrote something (even a partially
    // failed one -- whatever did commit is still worth reflecting) rebuilds
    // it in the background, same "skip on a no-op pass" gating as the
    // checkpoint above. Never blocks or affects index::run's own completion.
    if done > 0 {
        tokio::spawn(crate::compact_rebuild::rebuild(state));
    }
}

async fn run_inner(state: &AppState) -> Result<()> {
    let server = state.media_server()?;
    let mappings = state.mappings();
    let sections = server.sections().await?;

    // Only attempt libraries that overlap a configured path mapping — indexing
    // an unmapped library would mark every one of its items "unreachable" and
    // clutter stats with data the user hasn't set up a mount for.
    let mut mapped_titles = Vec::new();
    let mut items = Vec::new();
    for section in sections.iter() {
        if section.kind != "movie" && section.kind != "show" {
            continue;
        }
        let is_mapped = section
            .locations
            .iter()
            .any(|loc| paths::has_mapping_for(loc, &mappings));
        if !is_mapped {
            info!(
                "skipping library '{}' — no path mapping configured",
                section.title
            );
            continue;
        }
        mapped_titles.push(section.title.clone());
        match server.items(section).await {
            Ok(mut got) => items.append(&mut got),
            Err(e) => {
                warn!("could not list section {}: {e:#}", section.title);
                state.log_event(
                    LogLevel::Error,
                    format!("Could not list library '{}': {e:#}", section.title),
                );
            }
        }
    }

    // Drop stale rows for libraries that are no longer mapped (mapping deleted,
    // or renamed) so they stop showing up in stats forever.
    {
        let pool = state.pool.clone();
        let mapped_titles = mapped_titles.clone();
        let pruned = tokio::task::spawn_blocking(move || -> anyhow::Result<usize> {
            let conn = pool.get()?;
            db::prune_unmapped_libraries(&conn, &mapped_titles)
        })
        .await??;
        if pruned > 0 {
            info!("pruned {pruned} stale item(s) from unmapped libraries");
            state.log_event(
                LogLevel::Info,
                format!("Removed {pruned} indexed item(s) from libraries no longer mapped"),
            );
        }
    }

    {
        let mut st = state.index.locked();
        st.total = items.len();
    }

    // Cap individual per-item error log lines so one bad library can't flood
    // the activity log; the running error count is still shown in IndexStatus.
    const MAX_LOGGED_ERRORS: usize = 25;
    let mut logged_errors = 0usize;

    // Items someone requested a subtitle for: if nothing local turns up, the
    // server's own copy is read for these (one API call each, not for every item).
    let requested = {
        let pool = state.pool.clone();
        tokio::task::spawn_blocking(move || -> Result<_> {
            let conn = pool.get()?;
            db::requested_rating_keys(&conn)
        })
        .await?
        .unwrap_or_default()
    };

    for item in items {
        {
            let mut st = state.index.locked();
            st.current = Some(item.title.clone());
        }
        let remote = requested.contains(&item.rating_key);
        match index_one(state, &mappings, &item, remote, false).await {
            Ok(Outcome::Indexed) => bump(state, |s| s.done += 1),
            Ok(Outcome::Skipped) => bump(state, |s| s.skipped += 1),
            Err(e) => {
                warn!("failed to index {}: {e:#}", item.title);
                if logged_errors < MAX_LOGGED_ERRORS {
                    state.log_event(LogLevel::Error, format!("{}: {e:#}", item.title));
                    logged_errors += 1;
                } else if logged_errors == MAX_LOGGED_ERRORS {
                    state.log_event(LogLevel::Warn, "Further item errors suppressed this run…");
                    logged_errors += 1;
                }
                bump(state, |s| {
                    s.errors += 1;
                    s.last_error = Some(format!("{}: {e:#}", item.title));
                });
            }
        }
    }
    Ok(())
}

fn bump(state: &AppState, f: impl FnOnce(&mut crate::state::IndexStatus)) {
    f(&mut state.index.locked());
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
