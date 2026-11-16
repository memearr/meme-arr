//! Layers: pictures laid over the clip for part of its length -- a built-in shape (an
//! arrow, a ring), an uploaded sticker, or another clip. Each has a time on screen,
//! a place, a size, a turn and an opacity.
//!
//! ffmpeg draws them with `overlay`, so an export with layers is built as a filter
//! graph (`-filter_complex`) instead of a single `-vf` chain: the clip is sized, each
//! layer is laid over it in order, and the captions go on top of the lot.

use super::*;
use crate::sprite::{self, Shape};

/// Most layers one export may have.
pub const MAX_LAYERS: usize = 12;

/// Smallest and largest layer size, as a fraction of the picture's width.
pub const MIN_LAYER_SIZE: f32 = 0.01;
pub const MAX_LAYER_SIZE: f32 = 2.0;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LayerKind {
    /// A built-in shape (see `sprite::SHAPES`) in a colour (`#RRGGBB`, default orange).
    Shape {
        shape: String,
        #[serde(default)]
        color: Option<String>,
    },
    /// An uploaded picture or clip (see `overlays`), by its id.
    Media { asset: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Layer {
    /// The editor's name for it; not used by the export.
    #[serde(default)]
    pub id: String,
    #[serde(flatten)]
    pub kind: LayerKind,
    /// When it comes on, in milliseconds from the start of the clip.
    #[serde(default)]
    pub start_ms: i64,
    /// When it goes off; `None` is the end of the clip.
    #[serde(default)]
    pub end_ms: Option<i64>,
    /// Where its centre is, as fractions of the picture (0 left/top .. 1 right/bottom).
    #[serde(default = "half")]
    pub x: f32,
    #[serde(default = "half")]
    pub y: f32,
    /// Its width as a fraction of the picture's width.
    #[serde(default = "default_layer_size")]
    pub size: f32,
    /// Clockwise degrees.
    #[serde(default)]
    pub rotation: f32,
    /// 0 invisible .. 1 solid.
    #[serde(default = "one")]
    pub opacity: f32,
    /// Where the centre is over the clip, as keyframes (see [`Keyframe`]). When there are
    /// any, they replace `x` and `y`.
    #[serde(default)]
    pub path: Vec<Keyframe>,
}

fn half() -> f32 {
    0.5
}
fn one() -> f32 {
    1.0
}
fn default_layer_size() -> f32 {
    0.2
}

/// Why `layers` can't be used on a clip `clip_ms` long, or `None` if they can. Whether
/// an uploaded file exists is checked when the layers are resolved, not here.
pub fn validate_layers(layers: &[Layer], clip_ms: i64) -> Option<String> {
    if layers.len() > MAX_LAYERS {
        return Some(format!("at most {MAX_LAYERS} layers per clip"));
    }
    for (i, l) in layers.iter().enumerate() {
        let n = i + 1;
        let finite = [l.x, l.y, l.size, l.rotation, l.opacity]
            .iter()
            .all(|v| v.is_finite());
        if !finite {
            return Some(format!("layer {n}: a number is not usable"));
        }
        if !(MIN_LAYER_SIZE..=MAX_LAYER_SIZE).contains(&l.size) {
            return Some(format!(
                "layer {n}: size must be between {MIN_LAYER_SIZE} and {MAX_LAYER_SIZE}"
            ));
        }
        if l.start_ms < 0 || l.start_ms >= clip_ms {
            return Some(format!("layer {n}: it must start within the clip"));
        }
        if l.end_ms.is_some_and(|e| e <= l.start_ms) {
            return Some(format!("layer {n}: it must end after it starts"));
        }
        if let Some(why) = validate_path(&l.path, clip_ms) {
            return Some(format!("layer {n}: {why}"));
        }
        match &l.kind {
            LayerKind::Shape { shape, color } => {
                if Shape::parse(shape).is_none() {
                    return Some(format!("layer {n}: unknown shape {shape:?}"));
                }
                if color
                    .as_deref()
                    .is_some_and(|c| parse_hex_color(c).is_none())
                {
                    return Some(format!("layer {n}: the colour must be #RRGGBB"));
                }
            }
            LayerKind::Media { asset } => {
                if asset.trim().is_empty() {
                    return Some(format!("layer {n}: no picture chosen"));
                }
            }
        }
    }
    None
}

/// Where a layer's picture comes from once resolved, and how ffmpeg should read it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayerSource {
    pub path: PathBuf,
    /// A picture that moves (GIF, WebP, a clip) loops while on screen; a still is held.
    pub motion: bool,
}

/// What ffmpeg needs for an export with layers: the extra `-i` inputs, and the graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayerPlan {
    pub input_args: Vec<String>,
    /// The filter graph, ending in an output labelled `[out]`.
    pub graph: String,
}

/// The temp sprite files written for the shapes of one export; removed on drop.
#[derive(Default)]
pub struct SpriteFiles(pub Vec<PathBuf>);

impl Drop for SpriteFiles {
    fn drop(&mut self) {
        for p in &self.0 {
            let _ = std::fs::remove_file(p);
        }
    }
}

/// Write the PNG for every shape layer to `tmp_dir`; the media layers are looked up
/// with `media` (an overlay id to its file and whether it is a still).
pub fn resolve_layers(
    layers: &[Layer],
    tmp_dir: &Path,
    id: &str,
    media: &dyn Fn(&str) -> Option<LayerSource>,
) -> Result<(Vec<LayerSource>, SpriteFiles)> {
    let mut sources = Vec::with_capacity(layers.len());
    let mut files = SpriteFiles::default();
    for (i, l) in layers.iter().enumerate() {
        match &l.kind {
            LayerKind::Shape { shape, color } => {
                let shape = Shape::parse(shape)
                    .ok_or_else(|| anyhow::anyhow!("layer {}: unknown shape", i + 1))?;
                let rgb = color
                    .as_deref()
                    .and_then(parse_hex_color)
                    .unwrap_or(sprite::DEFAULT_COLOR);
                let path = tmp_dir.join(format!("{id}.layer{i}.png"));
                std::fs::write(&path, sprite::shape_png(shape, rgb))?;
                files.0.push(path.clone());
                sources.push(LayerSource {
                    path,
                    motion: false,
                });
            }
            LayerKind::Media { asset } => {
                let src = media(asset).ok_or_else(|| {
                    anyhow::anyhow!("layer {}: the picture {asset:?} is no longer there", i + 1)
                })?;
                sources.push(src);
            }
        }
    }
    Ok((sources, files))
}

/// Seconds as ffmpeg expressions want them.
fn secs(ms: i64) -> String {
    format!("{:.3}", ms as f64 / 1000.0)
}

/// The inputs and filter graph for `layers` over a clip `clip_ms` long that is sized
/// to `width`. `post` is what runs on the finished picture (captions, the GIF
/// palette); it may be empty.
pub fn plan_layers(
    layers: &[Layer],
    sources: &[LayerSource],
    width: u32,
    fps: u32,
    clip_ms: i64,
    post: &str,
) -> LayerPlan {
    let mut input_args = Vec::new();
    let mut graph = format!("[0:v]fps={fps},scale={width}:-2:flags=lanczos[b0]");
    for (i, (l, src)) in layers.iter().zip(sources).enumerate() {
        let n = i + 1;
        let start = l.start_ms.clamp(0, clip_ms);
        let end = l.end_ms.unwrap_or(clip_ms).clamp(start, clip_ms);
        // a picture that moves loops for as long as the clip does; a still is held
        input_args.extend(if src.motion {
            ["-stream_loop".to_string(), "-1".into()]
        } else {
            ["-loop".to_string(), "1".into()]
        });
        input_args.extend([
            "-t".into(),
            secs(clip_ms),
            "-i".into(),
            src.path.to_string_lossy().into_owned(),
        ]);

        let px = ((width as f32 * l.size).round() as u32).max(2);
        let mut prep = format!(
            "setpts=PTS-STARTPTS+{}/TB,format=rgba,scale={px}:-1:flags=lanczos",
            secs(start)
        );
        if l.rotation.rem_euclid(360.0).abs() > f32::EPSILON {
            let rad = l.rotation.to_radians();
            // a turned picture needs a larger frame; the corners stay transparent
            prep.push_str(&format!(
                ",rotate=a={rad:.5}:c=none:ow='rotw({rad:.5})':oh='roth({rad:.5})'"
            ));
        }
        let opacity = l.opacity.clamp(0.0, 1.0);
        if opacity < 1.0 {
            prep.push_str(&format!(",colorchannelmixer=aa={opacity:.3}"));
        }
        graph.push_str(&format!(";[{n}:v]{prep}[l{n}]"));
        // where its centre is: fixed, or following the path over the clip
        let path = sorted_path(&l.path);
        let (x_at, y_at) = if path.is_empty() {
            (
                format!("{:.4}", l.x.clamp(0.0, 1.0)),
                format!("{:.4}", l.y.clamp(0.0, 1.0)),
            )
        } else {
            (path_expr(&path, Axis::X), path_expr(&path, Axis::Y))
        };
        graph.push_str(&format!(
            ";[b{}][l{n}]overlay=x='main_w*({x_at})-overlay_w/2':y='main_h*({y_at})-overlay_h/2':\
             enable='between(t,{},{})':shortest=1[b{n}]",
            n - 1,
            secs(start),
            secs(end),
        ));
    }
    let tail = if post.is_empty() { "null" } else { post };
    graph.push_str(&format!(";[b{}]{tail}[out]", layers.len()));
    LayerPlan { input_args, graph }
}

/// The full ffmpeg argv for an export whose picture is built by a filter graph:
/// the clip first (seeked and trimmed), then each layer's input, the graph, and the
/// codec and privacy options as for any export.
#[allow(clippy::too_many_arguments)]
pub fn export_args_with_layers(
    file: &str,
    start_ms: i64,
    end_ms: i64,
    plan: &LayerPlan,
    keep_audio: bool,
    codec: &[String],
    title: Option<&str>,
    out: &str,
) -> Vec<String> {
    let dur = (end_ms - start_ms).max(0);
    let mut args: Vec<String> = vec![
        "-y".into(),
        "-v".into(),
        "error".into(),
        "-ss".into(),
        ms_to_secs(start_ms),
        "-t".into(),
        ms_to_secs(dur),
        "-i".into(),
        file.into(),
    ];
    args.extend(plan.input_args.iter().cloned());
    args.extend(["-filter_complex".into(), plan.graph.clone()]);
    args.extend(["-map".into(), "[out]".into()]);
    if keep_audio {
        args.extend(["-map".into(), "0:a:0?".into()]);
    }
    args.extend_from_slice(codec);
    args.extend(privacy_args(title));
    args.push(out.into());
    args
}

#[cfg(test)]
#[path = "layers_tests.rs"]
mod tests;
