//! Follow a spot of the picture through a clip, to give a layer or the caption text a path.

use super::*;
use std::sync::OnceLock;
use tokio::sync::Semaphore;

/// Following a spot decodes the whole stretch of the clip; at most this many run at once.
const MAX_CONCURRENT_TRACKS: usize = 2;

fn track_slots() -> &'static Semaphore {
    static SLOTS: OnceLock<Semaphore> = OnceLock::new();
    SLOTS.get_or_init(|| Semaphore::new(MAX_CONCURRENT_TRACKS))
}

fn default_size() -> f32 {
    0.12
}

fn default_max_points() -> usize {
    crate::export::MAX_PATH_POINTS
}

#[derive(Debug, Deserialize)]
pub(super) struct TrackBody {
    pub rating_key: String,
    /// The clip, in the media's own time.
    pub start_ms: i64,
    pub end_ms: i64,
    /// The moment to start from, in milliseconds from the start of the clip.
    #[serde(default)]
    pub t_ms: i64,
    /// Where the spot is at that moment, as fractions of the picture.
    pub x: f32,
    pub y: f32,
    /// About how wide the spot is, as a fraction of the picture's width.
    #[serde(default = "default_size")]
    pub size: f32,
    /// The most keyframes the answer may have (the editor leaves room for the ones it keeps).
    #[serde(default = "default_max_points")]
    pub max_points: usize,
}

/// `POST /api/track`: follow the spot at `(x, y)`, `t_ms` into the clip, forward through
/// the rest of it; the answer is a path (keyframes) the layer or text can take.
pub(super) async fn track_spot(
    State(st): State<AppState>,
    Json(b): Json<TrackBody>,
) -> Result<Response, AppError> {
    if !(b.x.is_finite() && b.y.is_finite() && b.size.is_finite())
        || !(0.0..=1.0).contains(&b.x)
        || !(0.0..=1.0).contains(&b.y)
    {
        return Ok(bad_request(
            "x and y must be fractions of the picture (0 to 1)",
        ));
    }
    if b.end_ms <= b.start_ms {
        return Ok(bad_request("end must be greater than start"));
    }
    if b.end_ms - b.start_ms > crate::tracker::MAX_TRACK_MS {
        return Ok(bad_request(format!(
            "only {} seconds of a clip can be followed at once",
            crate::tracker::MAX_TRACK_MS / 1000
        )));
    }
    if b.t_ms < 0 || b.t_ms > b.end_ms - b.start_ms {
        return Ok(bad_request("the starting moment must be within the clip"));
    }
    let (_m, file) = match resolve_source(&st, &b.rating_key).await? {
        Ok(found) => found,
        Err(resp) => return Ok(resp),
    };
    let _slot = track_slots().acquire().await.map_err(anyhow::Error::from)?;
    let req = crate::tracker::TrackRequest {
        t0_ms: b.t_ms,
        x: b.x,
        y: b.y,
        size: b.size,
        max_points: b.max_points,
    };
    let result = crate::tracker::track_clip(
        (&st.config.ffmpeg_bin, &st.config.ffprobe_bin),
        &file,
        (b.start_ms, b.end_ms),
        &req,
    )
    .await;
    match result {
        Ok(out) => Ok(Json(json!({
            "path": out.path,
            "tracked_ms": out.tracked_ms,
            "lost": out.lost,
        }))
        .into_response()),
        Err(e) => match e.downcast_ref::<crate::tracker::Refused>() {
            Some(refused) => Ok(bad_request(refused.to_string())),
            None => Err(e.into()),
        },
    }
}

#[cfg(test)]
#[path = "track_tests.rs"]
mod tests;
