//! The look of the picture: colour (brightness, contrast, saturation, black and white or
//! sepia), a mirror image, and fades from and to black.
//!
//! The colour and the mirror are applied to the clip's picture before anything is laid on
//! it, so layers and captions keep their own colours and don't flip; the fades are applied
//! to the finished picture, so they take the layers and captions with them.

use super::*;

/// A whole-picture tone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tone {
    /// The colours as they are.
    #[default]
    None,
    /// Black and white.
    Bw,
    /// Warm brown tones.
    Sepia,
}

fn zero() -> f32 {
    0.0
}
fn one() -> f32 {
    1.0
}

/// How the picture looks. The default is the picture as it is.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Grade {
    /// -1 (black) .. 0 (as it is) .. 1 (white): added to every pixel.
    #[serde(default = "zero")]
    pub brightness: f32,
    /// 0 (flat grey) .. 1 (as it is) .. 2 (twice the contrast).
    #[serde(default = "one")]
    pub contrast: f32,
    /// 0 (no colour) .. 1 (as it is) .. 3 (very vivid).
    #[serde(default = "one")]
    pub saturation: f32,
    #[serde(default)]
    pub tone: Tone,
    /// Left and right swapped (the picture only: captions and layers are not).
    #[serde(default)]
    pub mirror: bool,
    /// The picture comes up from black over this long, from the start of the clip.
    #[serde(default)]
    pub fade_in_ms: i64,
    /// The picture goes down to black over this long, ending at the end of the clip.
    #[serde(default)]
    pub fade_out_ms: i64,
}

impl Default for Grade {
    fn default() -> Self {
        Grade {
            brightness: 0.0,
            contrast: 1.0,
            saturation: 1.0,
            tone: Tone::None,
            mirror: false,
            fade_in_ms: 0,
            fade_out_ms: 0,
        }
    }
}

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.0005
}

/// Why `grade` can't be used on a `clip_ms` clip, or `None`.
pub fn validate_grade(grade: &Grade, clip_ms: i64) -> Option<String> {
    let in_range = |v: f32, lo: f32, hi: f32| v.is_finite() && (lo..=hi).contains(&v);
    if !in_range(grade.brightness, -1.0, 1.0) {
        return Some("the brightness must be between -1 and 1".into());
    }
    if !in_range(grade.contrast, 0.0, 2.0) {
        return Some("the contrast must be between 0 and 2".into());
    }
    if !in_range(grade.saturation, 0.0, 3.0) {
        return Some("the saturation must be between 0 and 3".into());
    }
    if grade.fade_in_ms < 0 || grade.fade_out_ms < 0 {
        return Some("a fade can't be negative".into());
    }
    if grade.fade_in_ms + grade.fade_out_ms > clip_ms {
        return Some("the fades together are longer than the clip".into());
    }
    None
}

impl Grade {
    /// Whether this leaves the picture as it is.
    pub fn is_plain(&self) -> bool {
        self.picture_filters().is_empty() && self.fade_filters(1).is_empty()
    }

    /// The filters for the clip's picture, to go right after it is sized (empty: none).
    pub fn picture_filters(&self) -> Vec<String> {
        let mut f = Vec::new();
        if !near(self.brightness, 0.0) || !near(self.contrast, 1.0) || !near(self.saturation, 1.0) {
            f.push(format!(
                "eq=brightness={:.3}:contrast={:.3}:saturation={:.3}",
                self.brightness, self.contrast, self.saturation
            ));
        }
        match self.tone {
            Tone::None => {}
            Tone::Bw => f.push("hue=s=0".into()),
            Tone::Sepia => {
                f.push("colorchannelmixer=.393:.769:.189:0:.349:.686:.168:0:.272:.534:.131".into())
            }
        }
        if self.mirror {
            f.push("hflip".into());
        }
        f
    }

    /// The fades, for a `clip_ms` clip, to go on the finished picture (empty: none).
    pub fn fade_filters(&self, clip_ms: i64) -> Vec<String> {
        let secs = |ms: i64| format!("{:.3}", ms as f64 / 1000.0);
        let mut f = Vec::new();
        if self.fade_in_ms > 0 {
            f.push(format!("fade=t=in:st=0:d={}", secs(self.fade_in_ms)));
        }
        if self.fade_out_ms > 0 {
            f.push(format!(
                "fade=t=out:st={}:d={}",
                secs((clip_ms - self.fade_out_ms).max(0)),
                secs(self.fade_out_ms)
            ));
        }
        f
    }
}

/// [`core_chain`] for a graded clip: the colour and mirror right after the sizing, the fades
/// after the captions.
pub fn core_chain_graded(
    width: u32,
    fps: u32,
    spec: &CaptionSpec,
    grade: &Grade,
    clip_ms: i64,
) -> String {
    let mut parts = vec![
        format!("fps={fps}"),
        format!("scale={width}:-2:flags=lanczos"),
    ];
    parts.extend(grade.picture_filters());
    parts.extend(caption_filters(spec));
    parts.extend(grade.fade_filters(clip_ms));
    parts.join(",")
}

/// The base of a layered export's graph (see `plan_layers_on`): sizing, then the colour and mirror.
pub fn layers_base_graded(width: u32, fps: u32, grade: &Grade) -> String {
    let mut parts = vec![
        format!("fps={fps}"),
        format!("scale={width}:-2:flags=lanczos"),
    ];
    parts.extend(grade.picture_filters());
    parts.join(",")
}

#[cfg(test)]
#[path = "grade_tests.rs"]
mod tests;
