//! A path: where something (a layer, the caption text) is over the length of the clip,
//! as keyframes. Between two keyframes it moves in a straight line; before the first and
//! after the last it stays where that keyframe puts it.
//!
//! A keyframe's time is in milliseconds from the start of the clip, its place the
//! centre as fractions of the picture (0 left/top .. 1 right/bottom), the same
//! convention as a layer's `x` and `y`.

use super::*;

/// Most keyframes one path may have. Each one costs a term in the expression ffmpeg
/// evaluates for every frame, so this is bounded (the tracker thins a long path to fit).
pub const MAX_PATH_POINTS: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Keyframe {
    pub t_ms: i64,
    pub x: f32,
    pub y: f32,
}

/// The path with unusable keyframes (a number that is not finite) dropped, kept
/// inside the picture, in time order, and with one keyframe per instant (the last one given wins).
pub fn sorted_path(path: &[Keyframe]) -> Vec<Keyframe> {
    let mut kept: Vec<Keyframe> = path
        .iter()
        .filter(|k| k.x.is_finite() && k.y.is_finite())
        .map(|k| Keyframe {
            t_ms: k.t_ms,
            x: k.x.clamp(0.0, 1.0),
            y: k.y.clamp(0.0, 1.0),
        })
        .collect();
    // a stable sort keeps the given order among equal times, so the last of them wins below
    kept.sort_by_key(|k| k.t_ms);
    let mut out: Vec<Keyframe> = Vec::with_capacity(kept.len());
    for k in kept {
        match out.last_mut() {
            Some(last) if last.t_ms == k.t_ms => *last = k,
            _ => out.push(k),
        }
    }
    out
}

/// Where the path puts the thing `t_ms` into the clip; `fallback` when there is no path.
/// `path` must be sorted (see [`sorted_path`]).
pub fn position_at(path: &[Keyframe], t_ms: i64, fallback: (f32, f32)) -> (f32, f32) {
    let (Some(first), Some(last)) = (path.first(), path.last()) else {
        return fallback;
    };
    if t_ms <= first.t_ms {
        return (first.x, first.y);
    }
    if t_ms >= last.t_ms {
        return (last.x, last.y);
    }
    let i = path.partition_point(|k| k.t_ms <= t_ms);
    let (a, b) = (path[i - 1], path[i]);
    let f = (t_ms - a.t_ms) as f32 / (b.t_ms - a.t_ms) as f32;
    (a.x + (b.x - a.x) * f, a.y + (b.y - a.y) * f)
}

/// Why `path` can't be used on a clip `clip_ms` long, or `None` if it can.
pub fn validate_path(path: &[Keyframe], clip_ms: i64) -> Option<String> {
    if path.len() > MAX_PATH_POINTS {
        return Some(format!(
            "a path can have at most {MAX_PATH_POINTS} keyframes"
        ));
    }
    for k in path {
        if !(k.x.is_finite() && k.y.is_finite()) {
            return Some("a keyframe has a number that is not usable".into());
        }
        if !(0.0..=1.0).contains(&k.x) || !(0.0..=1.0).contains(&k.y) {
            return Some("a keyframe must be inside the picture (0 to 1)".into());
        }
        if k.t_ms < 0 || k.t_ms > clip_ms {
            return Some("a keyframe must be within the clip".into());
        }
    }
    None
}

/// Which way across the picture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    X,
    Y,
}

/// An ffmpeg expression of `t` (seconds into the clip) that is the path's `axis` value:
/// straight lines between keyframes, held at the ends. `path` must be sorted and
/// not empty. Commas in it are inside a quoted filter option, as `overlay` is given it.
///
/// The choice of which line applies is a balanced tree of `if`s (ffmpeg's expression
/// parser refuses to nest much deeper than a hundred levels, and a chain of
/// `MAX_PATH_POINTS` of them would be that deep); here it is about log2(n) deep.
pub fn path_expr(path: &[Keyframe], axis: Axis) -> String {
    let val = |k: &Keyframe| match axis {
        Axis::X => k.x as f64,
        Axis::Y => k.y as f64,
    };
    let secs = |k: &Keyframe| k.t_ms as f64 / 1000.0;
    match path {
        [] => "0.5".into(),
        [only] => format!("{:.5}", val(only)),
        _ => {
            // the value on the line from keyframe a to b, held outside it by clip()
            let seg = |a: &Keyframe, b: &Keyframe| {
                format!(
                    "{:.5}+({:.5})*(clip(t,{:.3},{:.3})-{:.3})/{:.3}",
                    val(a),
                    val(b) - val(a),
                    secs(a),
                    secs(b),
                    secs(a),
                    secs(b) - secs(a)
                )
            };
            // lines lo..hi, where line i runs from keyframe i to keyframe i + 1
            fn tree(
                path: &[Keyframe],
                lo: usize,
                hi: usize,
                seg: &dyn Fn(&Keyframe, &Keyframe) -> String,
                secs: &dyn Fn(&Keyframe) -> f64,
            ) -> String {
                if hi - lo == 1 {
                    return seg(&path[lo], &path[lo + 1]);
                }
                let mid = lo + (hi - lo) / 2;
                format!(
                    "if(lt(t,{:.3}),{},{})",
                    secs(&path[mid]),
                    tree(path, lo, mid, seg, secs),
                    tree(path, mid, hi, seg, secs)
                )
            }
            tree(path, 0, path.len() - 1, &seg, &secs)
        }
    }
}

#[cfg(test)]
#[path = "path_tests.rs"]
mod tests;
