//! Background build/load of the research compact dialogue index (see
//! `compactindex`). Shadow-mode only — nothing here ever changes what a
//! user's search actually returns; see `AppState::compact`'s doc comment
//! and the search handler's shadow check in `api::search`.

use crate::compactindex::{CompactIndex, PRODUCTION_BLOCK_SIZE};
use crate::state::{AppState, LogLevel};
use crate::sync::RwLockExt;
use tracing::{info, warn};

/// Try loading a previously-saved index first (fast — no need to re-walk
/// `subtitle_cue` on every restart); fall back to a full rebuild if that
/// fails for any reason (no file yet, corrupt, a `COMPACT_INDEX_FORMAT_VERSION`
/// bump). Call via `tokio::spawn` — both paths do real I/O/CPU work and must
/// never block server startup.
pub async fn load_or_rebuild(state: AppState) {
    let path = state.config.compact_index_path();
    let loaded = tokio::task::spawn_blocking(move || CompactIndex::load_from(&path)).await;

    match loaded {
        Ok(Ok(idx)) => {
            info!(
                "compact dialogue index loaded from disk: {} rows, {} terms",
                idx.row_count(),
                idx.term_count()
            );
            *state.compact.write_locked() = Some(idx);
        }
        Ok(Err(e)) => {
            info!("no usable compact index on disk ({e:#}), rebuilding");
            rebuild(state).await;
        }
        Err(e) => {
            warn!("compact index load task panicked: {e}");
        }
    }
}

/// Rebuild from `subtitle_cue` and persist the result, replacing whatever
/// was loaded before. `subtitle_cue` only ever changes inside `index::run`
/// (inserts, delete-then-reinsert on reprocessing, and library pruning all
/// happen there), so that's the one place this needs triggering from — see
/// its call at the end of `index::run`. This is always a *full* rebuild,
/// never an in-place patch: see the module docs on `compactindex` for why
/// (append-only postings can't un-append a deleted row).
pub async fn rebuild(state: AppState) {
    let pool = state.pool.clone();
    let path = state.config.compact_index_path();

    let result = tokio::task::spawn_blocking(move || -> anyhow::Result<CompactIndex> {
        let conn = pool.get()?;
        let idx = CompactIndex::rebuild_from_db(&conn, PRODUCTION_BLOCK_SIZE)?;
        if let Err(e) = idx.save_to(&path) {
            // Not fatal: the in-memory index built just fine and is still
            // usable this run, it just won't be there as a fast-load on the
            // next restart.
            warn!("compact index built but failed to save to disk: {e:#}");
        }
        Ok(idx)
    })
    .await;

    match result {
        Ok(Ok(idx)) => {
            info!(
                "compact dialogue index rebuilt: {} rows, {} terms, {} bytes",
                idx.row_count(),
                idx.term_count(),
                idx.size_bytes()
            );
            state.log_event(
                LogLevel::Info,
                format!(
                    "Compact dialogue index rebuilt ({} rows, {} terms)",
                    idx.row_count(),
                    idx.term_count()
                ),
            );
            *state.compact.write_locked() = Some(idx);
        }
        Ok(Err(e)) => {
            warn!("compact index rebuild failed: {e:#}");
            state.log_event(
                LogLevel::Warn,
                format!("Compact index rebuild failed: {e:#}"),
            );
        }
        Err(e) => {
            warn!("compact index rebuild task panicked: {e}");
        }
    }
}
