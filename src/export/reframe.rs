//! Speed and reframing: how fast the finished clip plays, and which window of the picture it keeps.
//!
//! Both are the last things done to the clip, after the captions, layers and fades are on it, so
//! everything in the editor is placed on the full picture at normal speed and then sped up or cropped
//! as one. A caption stays in step with the speech it was timed to, and a layer's times are still
//! the clip's own. The sound is sped up the same way (keeping its pitch).

use super::*;

/// Slowest and fastest a clip may play.
pub const MIN_SPEED: f32 = 0.25;
pub const MAX_SPEED: f32 = 4.0;
/// The smallest share of the picture's width or height a crop may keep.
pub const MIN_CROP: f32 = 0.2;

/// A window onto the finished picture, as fractions of it: `x`, `y` is the top-left corner (0 left/top)
/// and `w`, `h` the size (1 is all of it). The result is the shape of the window, at the export's width.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Crop {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// Why `speed` can't be used, or `None`.
pub fn validate_speed(speed: f32) -> Option<String> {
    if !speed.is_finite() || !(MIN_SPEED..=MAX_SPEED).contains(&speed) {
        return Some(format!(
            "the speed must be between {MIN_SPEED} and {MAX_SPEED} times"
        ));
    }
    None
}

/// Why `crop` can't be used, or `None`.
pub fn validate_crop(crop: Option<&Crop>) -> Option<String> {
    let c = crop?;
    let sane = [c.x, c.y, c.w, c.h].iter().all(|v| v.is_finite());
    if !sane || c.x < 0.0 || c.y < 0.0 {
        return Some("the crop window must be inside the picture".into());
    }
    if c.w < MIN_CROP || c.h < MIN_CROP {
        return Some(format!(
            "the crop window must keep at least {}% of the picture each way",
            (MIN_CROP * 100.0) as u32
        ));
    }
    if c.x + c.w > 1.0005 || c.y + c.h > 1.0005 {
        return Some("the crop window must be inside the picture".into());
    }
    None
}

/// Whether `speed` leaves the speed as it is.
pub fn is_normal_speed(speed: f32) -> bool {
    (speed - 1.0).abs() < 0.0005
}

/// The width to put the whole picture at so that the cropped window comes out `width` wide.
/// Even, and the same as `width` with no crop.
pub fn compose_width(width: u32, crop: Option<&Crop>) -> u32 {
    let full = match crop {
        Some(c) if c.w > 0.0 => (width as f32 / c.w).round() as u32,
        _ => width,
    };
    (full - full % 2).max(2)
}

/// The filters that finish the picture: the crop (and sizing the window to `width`), then the speed.
/// Empty when there is nothing to do.
pub fn finishing_filters(width: u32, fps: u32, crop: Option<&Crop>, speed: f32) -> Vec<String> {
    let mut f = Vec::new();
    if let Some(c) = crop {
        f.push(format!(
            "crop=trunc(iw*{:.4}/2)*2:trunc(ih*{:.4}/2)*2:trunc(iw*{:.4}):trunc(ih*{:.4})",
            c.w, c.h, c.x, c.y
        ));
        f.push(format!("scale={width}:-2:flags=lanczos"));
    }
    if !is_normal_speed(speed) {
        f.push(format!("setpts=PTS/{speed:.4}"));
        f.push(format!("fps={fps}"));
    }
    f
}

/// The sound at `speed` times, pitch kept: `atempo` takes 0.5 to 2 at a time, so a larger change is
/// chained. A leading comma, or "" at normal speed.
pub fn tempo_filter(speed: f32) -> String {
    if is_normal_speed(speed) || !speed.is_finite() || speed <= 0.0 {
        return String::new();
    }
    let mut rest = speed as f64;
    let mut parts = Vec::new();
    while rest > 2.0 {
        parts.push("atempo=2.0".to_string());
        rest /= 2.0;
    }
    while rest < 0.5 {
        parts.push("atempo=0.5".to_string());
        rest /= 0.5;
    }
    parts.push(format!("atempo={rest:.4}"));
    format!(",{}", parts.join(","))
}

#[cfg(test)]
#[path = "reframe_tests.rs"]
mod tests;
