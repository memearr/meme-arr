//! Follow a spot of the picture through a clip, to give a layer or the caption text a
//! path (see `export::path`) without placing every keyframe by hand.
//!
//! The clip is decoded by ffmpeg to small greyscale frames. A patch of the picture round
//! the chosen spot is the template; in each frame it is searched for near where it was
//! last, by zero-mean normalised cross-correlation (so a change of brightness does not
//! throw it), to a fraction of a pixel. The template is slowly updated while the match is
//! good, so it keeps up as the thing turns or moves toward the camera; when the match is
//! lost for a few frames in a row, tracking stops and says so. The positions, one per
//! frame, are then thinned to the few keyframes that describe the same movement.
//!
//! No computer-vision library is involved: this is some arithmetic on bytes.

use crate::export::{position_at, Keyframe, MAX_PATH_POINTS};
use crate::ffmpeg;
use anyhow::Result;
use std::path::Path;

/// A request that cannot be followed, for a reason the person asking can fix. Anything
/// else that goes wrong (ffmpeg failing, an unreadable file) is an ordinary error.
#[derive(Debug)]
pub struct Refused(pub String);

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for Refused {}

fn refuse<T>(why: impl Into<String>) -> Result<T> {
    Err(Refused(why.into()).into())
}

/// Tracking is done on frames whose longer side is this many pixels.
pub const TRACK_SIDE: usize = 320;
/// Frames per second tracked at.
pub const TRACK_FPS: u32 = 12;
/// The longest stretch of clip that is tracked at once.
pub const MAX_TRACK_MS: i64 = 60_000;

/// A match worse than this counts as not having found it.
const LOST_BELOW: f32 = 0.45;
/// This many frames in a row not found ends the tracking.
const LOST_FRAMES: usize = 3;
/// The template is updated only from a match at least this good.
const UPDATE_ABOVE: f32 = 0.85;
/// How much of the new patch goes into the template when it is updated.
const UPDATE_RATE: f32 = 0.15;
/// A template flatter than this (standard deviation, grey levels) has nothing to hold on to.
const MIN_TEMPLATE_DEVIATION: f32 = 3.0;

/// A square patch of grey levels, with its mean taken off.
struct Template {
    size: usize,
    /// The patch minus its mean.
    data: Vec<f32>,
}

impl Template {
    fn from_frame(frame: &[u8], w: usize, x: usize, y: usize, size: usize) -> Template {
        let mut data = Vec::with_capacity(size * size);
        for r in 0..size {
            let row = &frame[(y + r) * w + x..(y + r) * w + x + size];
            data.extend(row.iter().map(|&p| p as f32));
        }
        let mean = data.iter().sum::<f32>() / data.len() as f32;
        data.iter_mut().for_each(|v| *v -= mean);
        Template { size, data }
    }

    /// The standard deviation of the grey levels in the patch.
    fn deviation(&self) -> f32 {
        (self.data.iter().map(|v| v * v).sum::<f32>() / self.data.len() as f32).sqrt()
    }

    fn norm(&self) -> f32 {
        self.data.iter().map(|v| v * v).sum::<f32>().sqrt()
    }

    /// Move the template a little toward the patch at (x, y).
    fn blend(&mut self, frame: &[u8], w: usize, x: usize, y: usize) {
        let patch = Template::from_frame(frame, w, x, y, self.size);
        for (t, p) in self.data.iter_mut().zip(&patch.data) {
            *t += (p - *t) * UPDATE_RATE;
        }
    }
}

/// How well the template matches the patch of `frame` whose top-left is (x, y): -1 to 1.
fn zncc(frame: &[u8], w: usize, x: usize, y: usize, t: &Template, t_norm: f32) -> f32 {
    let n = (t.size * t.size) as f32;
    let (mut sum_p, mut sum_pp, mut sum_pt) = (0f32, 0f32, 0f32);
    for r in 0..t.size {
        let row = &frame[(y + r) * w + x..(y + r) * w + x + t.size];
        let trow = &t.data[r * t.size..(r + 1) * t.size];
        for (&p, &tv) in row.iter().zip(trow) {
            let p = p as f32;
            sum_p += p;
            sum_pp += p * p;
            sum_pt += p * tv;
        }
    }
    let var = sum_pp - sum_p * sum_p / n;
    if var <= 1.0 || t_norm <= f32::EPSILON {
        return 0.0;
    }
    sum_pt / (var.sqrt() * t_norm)
}

/// Where the spot was in one frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    pub frame: usize,
    /// The centre of the template, in pixels of the tracked frame (a fraction of one
    /// where the match falls between pixels).
    pub x: f32,
    pub y: f32,
    pub score: f32,
}

#[derive(Debug, Clone, Copy)]
pub struct TrackParams {
    /// The template's side, in pixels.
    pub template: usize,
    /// How far from where it was the spot is looked for, in pixels.
    pub radius: usize,
}

/// What came of following a spot.
#[derive(Debug, Clone, PartialEq)]
pub struct Followed {
    pub points: Vec<Point>,
    /// The spot was lost before the frames ran out.
    pub lost: bool,
}

/// Follow the spot centred at `start` (pixels) in frame `first`, through the frames
/// after it. `frames` is `count` frames of `w * h` greyscale bytes one after another.
pub fn follow(
    frames: &[u8],
    (w, h): (usize, usize),
    first: usize,
    start: (f32, f32),
    params: TrackParams,
) -> Result<Followed> {
    let frame_len = w * h;
    let count = frames.len().checked_div(frame_len).unwrap_or(0);
    if first >= count {
        return refuse("there is no picture at that moment to follow");
    }
    let size = params.template.min(w).min(h).max(8);
    let frame = |i: usize| &frames[i * frame_len..(i + 1) * frame_len];
    // the template's top-left that puts its centre at (cx, cy), kept inside the picture
    let top_left = |cx: f32, cy: f32| {
        let max_x = (w - size) as f32;
        let max_y = (h - size) as f32;
        (
            (cx - size as f32 / 2.0).round().clamp(0.0, max_x) as usize,
            (cy - size as f32 / 2.0).round().clamp(0.0, max_y) as usize,
        )
    };

    let (tx, ty) = top_left(start.0, start.1);
    let mut template = Template::from_frame(frame(first), w, tx, ty, size);
    if template.deviation() < MIN_TEMPLATE_DEVIATION {
        return refuse(
            "there is nothing to follow at that spot (the picture is flat there); pick a spot with some detail",
        );
    }
    let half = size as f32 / 2.0;
    let mut points = vec![Point {
        frame: first,
        x: tx as f32 + half,
        y: ty as f32 + half,
        score: 1.0,
    }];
    // the top-left of the last match, and how it was moving
    let mut at = (tx as f32, ty as f32);
    let mut velocity = (0f32, 0f32);
    let mut missed = 0usize;
    let mut lost = false;

    for i in first + 1..count {
        let img = frame(i);
        let norm = template.norm();
        // look near where it was, allowing for how it was moving
        let guess = (at.0 + velocity.0 * 0.5, at.1 + velocity.1 * 0.5);
        let max_x = (w - size) as isize;
        let max_y = (h - size) as isize;
        let (gx, gy) = (guess.0.round() as isize, guess.1.round() as isize);
        let r = params.radius as isize;
        let (x0, x1) = ((gx - r).clamp(0, max_x), (gx + r).clamp(0, max_x));
        let (y0, y1) = ((gy - r).clamp(0, max_y), (gy + r).clamp(0, max_y));

        // coarse: every other position; then the eight round the best
        let mut best = (f32::MIN, x0, y0);
        let mut y = y0;
        while y <= y1 {
            let mut x = x0;
            while x <= x1 {
                let s = zncc(img, w, x as usize, y as usize, &template, norm);
                if s > best.0 {
                    best = (s, x, y);
                }
                x += 2;
            }
            y += 2;
        }
        let (mut bs, mut bx, mut by) = best;
        for dy in -1..=1isize {
            for dx in -1..=1isize {
                let (x, y) = (bx + dx, by + dy);
                if (dx, dy) == (0, 0) || x < 0 || y < 0 || x > max_x || y > max_y {
                    continue;
                }
                let s = zncc(img, w, x as usize, y as usize, &template, norm);
                if s > bs {
                    bs = s;
                    (bx, by) = (x, y);
                }
            }
        }

        if bs < LOST_BELOW {
            missed += 1;
            if missed >= LOST_FRAMES {
                lost = true;
                break;
            }
            // hold where it was until it is found again
            points.push(Point {
                frame: i,
                x: at.0 + half,
                y: at.1 + half,
                score: bs,
            });
            continue;
        }
        missed = 0;

        // a fraction of a pixel: the top of the parabola through the match and its neighbours
        let sub = |lo: f32, mid: f32, hi: f32| {
            let d = lo - 2.0 * mid + hi;
            if d.abs() < 1e-6 {
                0.0
            } else {
                (0.5 * (lo - hi) / d).clamp(-0.5, 0.5)
            }
        };
        let score_at = |x: isize, y: isize| {
            if x < 0 || y < 0 || x > max_x || y > max_y {
                bs
            } else {
                zncc(img, w, x as usize, y as usize, &template, norm)
            }
        };
        let fx = sub(score_at(bx - 1, by), bs, score_at(bx + 1, by));
        let fy = sub(score_at(bx, by - 1), bs, score_at(bx, by + 1));
        let found = (bx as f32 + fx, by as f32 + fy);
        velocity = (found.0 - at.0, found.1 - at.1);
        at = found;
        points.push(Point {
            frame: i,
            x: at.0 + half,
            y: at.1 + half,
            score: bs,
        });
        if bs >= UPDATE_ABOVE {
            template.blend(img, w, bx as usize, by as usize);
        }
    }
    Ok(Followed { points, lost })
}

/// The few keyframes that describe the same movement as `points` (one per frame, at
/// `fps`): the positions are smoothed a little, then every point that a straight line
/// between its neighbours already passes within `tolerance` (a fraction of the picture)
/// is dropped (and never more than `limit` keyframes are kept). `t0_ms` is the time of the
/// frame the points start from, in the clip.
pub fn to_path(
    points: &[Point],
    (w, h): (usize, usize),
    fps: u32,
    t0_ms: i64,
    tolerance: f32,
    limit: usize,
) -> Vec<Keyframe> {
    if points.is_empty() {
        return Vec::new();
    }
    let at = |p: &Point| Keyframe {
        t_ms: t0_ms + ((p.frame - points[0].frame) as i64 * 1000) / fps as i64,
        x: (p.x / w as f32).clamp(0.0, 1.0),
        y: (p.y / h as f32).clamp(0.0, 1.0),
    };
    let raw: Vec<Keyframe> = points.iter().map(at).collect();
    // a 3-point moving average takes the jitter out, and keeps the ends where they are
    let mut smooth = raw.clone();
    for i in 1..raw.len().saturating_sub(1) {
        smooth[i].x = 0.25 * raw[i - 1].x + 0.5 * raw[i].x + 0.25 * raw[i + 1].x;
        smooth[i].y = 0.25 * raw[i - 1].y + 0.5 * raw[i].y + 0.25 * raw[i + 1].y;
    }
    simplify(&smooth, tolerance, limit)
}

/// Thin a path to the keyframes needed to stay within `tolerance` of it, and never more than
/// `limit` (at most `MAX_PATH_POINTS`, at least 2; the tolerance is relaxed until it fits).
pub fn simplify(path: &[Keyframe], tolerance: f32, limit: usize) -> Vec<Keyframe> {
    let limit = limit.clamp(2, MAX_PATH_POINTS);
    let mut tol = tolerance.max(1e-4);
    loop {
        let mut keep = vec![false; path.len()];
        if let Some(first) = keep.first_mut() {
            *first = true;
        }
        if let Some(last) = keep.last_mut() {
            *last = true;
        }
        let mut stack = vec![(0usize, path.len().saturating_sub(1))];
        while let Some((lo, hi)) = stack.pop() {
            if hi <= lo + 1 {
                continue;
            }
            let ends = [path[lo], path[hi]];
            // the point the straight line between the ends misses by most
            let mut worst = (0f32, lo);
            for (i, k) in path.iter().enumerate().take(hi).skip(lo + 1) {
                let (lx, ly) = position_at(&ends, k.t_ms, (k.x, k.y));
                let miss = (k.x - lx).hypot(k.y - ly);
                if miss > worst.0 {
                    worst = (miss, i);
                }
            }
            if worst.0 > tol {
                keep[worst.1] = true;
                stack.push((lo, worst.1));
                stack.push((worst.1, hi));
            }
        }
        let out: Vec<Keyframe> = path
            .iter()
            .zip(&keep)
            .filter(|(_, k)| **k)
            .map(|(p, _)| *p)
            .collect();
        if out.len() <= limit {
            return out;
        }
        tol *= 1.5;
    }
}

/// What to follow: a spot at (x, y) -- fractions of the picture -- at `t0_ms` into the clip,
/// about `size` of the picture's width across.
#[derive(Debug, Clone, Copy)]
pub struct TrackRequest {
    pub t0_ms: i64,
    pub x: f32,
    pub y: f32,
    pub size: f32,
    /// The most keyframes the answer may have.
    pub max_points: usize,
}

/// What following the spot came to.
#[derive(Debug, Clone, PartialEq)]
pub struct TrackOutcome {
    /// The keyframes, from `t0_ms` on, in the clip's own time.
    pub path: Vec<Keyframe>,
    /// How much of the clip, from `t0_ms`, was followed.
    pub tracked_ms: i64,
    /// The spot was lost before the clip ended.
    pub lost: bool,
}

/// The size to decode at: the longer side `TRACK_SIDE`, both even.
pub fn track_dims(width: u32, height: u32) -> (usize, usize) {
    let (w, h) = (width.max(2) as f64, height.max(2) as f64);
    let k = TRACK_SIDE as f64 / w.max(h);
    let even = |v: f64| (((v * k) / 2.0).round() as usize * 2).max(2);
    (even(w), even(h))
}

/// Follow a spot through the clip `start_ms..end_ms` of `file`.
pub async fn track_clip(
    tools: (&str, &str),
    file: &Path,
    (start_ms, end_ms): (i64, i64),
    req: &TrackRequest,
) -> Result<TrackOutcome> {
    let (ffmpeg_bin, ffprobe_bin) = tools;
    if end_ms <= start_ms {
        return refuse("end must be after start");
    }
    if end_ms - start_ms > MAX_TRACK_MS {
        return refuse(format!(
            "only {} seconds of a clip can be followed at once",
            MAX_TRACK_MS / 1000
        ));
    }
    let info = ffmpeg::probe_video(ffprobe_bin, file).await?;
    let (w, h) = track_dims(info.width, info.height);
    let args: Vec<String> = [
        "-v",
        "error",
        "-ss",
        &format!("{:.3}", start_ms as f64 / 1000.0),
        "-t",
        &format!("{:.3}", (end_ms - start_ms) as f64 / 1000.0),
        "-i",
        &file.to_string_lossy(),
        "-an",
        "-sn",
        "-vf",
        &format!("fps={TRACK_FPS},scale={w}:{h}:flags=area"),
        "-pix_fmt",
        "gray",
        "-f",
        "rawvideo",
        "-",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let frames = ffmpeg::run_capture(ffmpeg_bin, &args).await?;
    let req = *req;
    tokio::task::spawn_blocking(move || track_decoded(&frames, (w, h), &req)).await?
}

/// [`track_clip`] on frames that are already decoded.
pub fn track_decoded(
    frames: &[u8],
    (w, h): (usize, usize),
    req: &TrackRequest,
) -> Result<TrackOutcome> {
    let count = frames.len() / (w * h);
    if count == 0 {
        return refuse("no picture could be read from that part of the clip");
    }
    let first = ((req.t0_ms.max(0) * TRACK_FPS as i64 + 500) / 1000) as usize;
    let first = first.min(count - 1);
    let template = ((req.size.clamp(0.02, 0.5) * w as f32).round() as usize).clamp(12, 96);
    let followed = follow(
        frames,
        (w, h),
        first,
        (
            req.x.clamp(0.0, 1.0) * w as f32,
            req.y.clamp(0.0, 1.0) * h as f32,
        ),
        TrackParams {
            template,
            // a fast thing crosses about this much of the picture between frames
            radius: (TRACK_SIDE / 12).max(8),
        },
    )?;
    let t0 = first as i64 * 1000 / TRACK_FPS as i64;
    let path = to_path(
        &followed.points,
        (w, h),
        TRACK_FPS,
        t0,
        0.004,
        req.max_points,
    );
    let last = followed.points.last().map_or(first, |p| p.frame);
    Ok(TrackOutcome {
        path,
        tracked_ms: (last - first) as i64 * 1000 / TRACK_FPS as i64,
        lost: followed.lost,
    })
}

#[cfg(test)]
#[path = "tracker_tests.rs"]
mod tests;
