//! Layers on the clips of a sequence, rendered by the real ffmpeg: each clip carries its own,
//! placed on the shared canvas and timed from the start of that clip, with the sound and the
//! captions as before. Skips when ffmpeg is not installed.

use meme_arr::export::{
    self, Caption, Format, Layer, LayerKind, LayerSource, ResolvedSequenceClip, SequenceClipSpec,
    SequenceExportRequest,
};
use std::path::{Path, PathBuf};
use std::process::Command;

const TOOLS: export::Tools<'static> = export::Tools {
    ffmpeg: "ffmpeg",
    ffprobe: "ffprobe",
    gifsicle: "gifsicle",
};
const FONT: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/MemeTestSans.ttf"
);

/// The canvas of a sequence 320 wide.
const W: usize = 320;
const H: usize = 180;

macro_rules! need_ffmpeg {
    () => {
        if !Command::new("ffmpeg")
            .arg("-version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
        {
            eprintln!("skipping sequence layers test: ffmpeg not found on PATH");
            return;
        }
    };
}

/// A 4 s, 320x240 flat grey clip, with a tone when `audio`.
fn source(dir: &Path, name: &str, audio: bool) -> PathBuf {
    let out = dir.join(format!("{name}.mp4"));
    let mut cmd = Command::new("ffmpeg");
    cmd.args(["-y", "-v", "error", "-f", "lavfi", "-i"])
        .arg("color=c=0x303030:duration=4:size=320x240:rate=10");
    if audio {
        cmd.args(["-f", "lavfi", "-i", "sine=frequency=440:duration=4"]);
    }
    cmd.args(["-c:v", "libx264", "-pix_fmt", "yuv420p"]);
    if audio {
        cmd.args(["-c:a", "aac", "-shortest"]);
    }
    assert!(cmd.arg(&out).status().expect("spawn ffmpeg").success());
    out
}

fn shape(name: &str, x: f32, y: f32) -> Layer {
    Layer {
        id: "l".into(),
        kind: LayerKind::Shape {
            shape: name.into(),
            color: None,
        },
        start_ms: 1000,
        end_ms: Some(2000),
        x,
        y,
        size: 0.3,
        rotation: 0.0,
        opacity: 1.0,
        path: vec![],
    }
}

fn clip(file: &Path, layers: Vec<Layer>, caption: Option<Caption>) -> ResolvedSequenceClip {
    ResolvedSequenceClip {
        file: file.to_path_buf(),
        start_ms: 0,
        end_ms: 4000,
        caption,
        subtitle_offset_ms: 0,
        cues: vec![],
        source: String::new(),
        font_path: None,
        layers,
    }
}

fn request(audio: bool) -> SequenceExportRequest {
    SequenceExportRequest {
        clips: vec![],
        format: Format::Mp4,
        width: W as u32,
        fps: 10,
        quality: 95,
        audio,
        filename: None,
        credit_source: false,
        music: None,
        clip_volume: 1.0,
    }
}

async fn render(
    dir: &Path,
    name: &str,
    clips: &[ResolvedSequenceClip],
    audio: bool,
    media: &(dyn Fn(&str) -> Option<LayerSource> + Sync),
) -> anyhow::Result<PathBuf> {
    let out = dir.join(format!("{name}.mp4"));
    export::run_sequence_export_with(
        TOOLS,
        export::JobSpec {
            id: &format!("job-{name}"),
            tmp_dir: dir,
            out_path: &out,
            source_title: None,
        },
        &request(audio),
        clips,
        FONT,
        media,
    )
    .await?;
    Ok(out)
}

fn no_media(_: &str) -> Option<LayerSource> {
    None
}

/// The frame at `at_s` as `W * H * 3` RGB bytes.
fn frame(file: &Path, at_s: f64) -> Vec<u8> {
    let out = Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(file)
        .args(["-vf", "fps=10", "-ss", &format!("{at_s}")])
        .args(["-frames:v", "1", "-f", "rawvideo", "-pix_fmt", "rgb24", "-"])
        .output()
        .expect("spawn ffmpeg");
    assert!(out.status.success());
    assert_eq!(out.stdout.len(), W * H * 3, "the canvas is {W}x{H}");
    out.stdout
}

/// How many pixels are clearly orange (the default shape colour), and their mean x.
fn orange(f: &[u8]) -> (usize, f64) {
    let (mut n, mut sx) = (0usize, 0f64);
    for y in 0..H {
        for x in 0..W {
            let i = (y * W + x) * 3;
            let (r, g, b) = (f[i], f[i + 1], f[i + 2]);
            if r > 200 && (40..130).contains(&g) && b < 60 {
                n += 1;
                sx += x as f64;
            }
        }
    }
    (n, if n == 0 { 0.0 } else { sx / n as f64 })
}

/// How many pixels are near white (caption text) in the top third.
fn white_on_top(f: &[u8]) -> usize {
    (0..H / 3)
        .flat_map(|y| (0..W).map(move |x| (y * W + x) * 3))
        .filter(|&i| f[i] > 230 && f[i + 1] > 230 && f[i + 2] > 230)
        .count()
}

fn audio_secs(file: &Path) -> Option<f64> {
    let out = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "a",
            "-show_entries",
            "stream=duration",
        ])
        .args(["-of", "csv=p=0"])
        .arg(file)
        .output()
        .expect("spawn ffprobe");
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s.parse().unwrap_or(0.0))
    }
}

#[tokio::test]
async fn each_clip_has_its_own_layers_timed_from_its_own_start() {
    need_ffmpeg!();
    let tmp = tempfile::tempdir().unwrap();
    let (a, b) = (
        source(tmp.path(), "a", false),
        source(tmp.path(), "b", false),
    );
    let clips = [
        clip(&a, vec![shape("dot", 0.25, 0.5)], None),
        clip(&b, vec![shape("dot", 0.75, 0.5)], None),
    ];
    let out = render(tmp.path(), "two", &clips, false, &no_media)
        .await
        .unwrap();
    // clip 1 is 0..4 s, clip 2 is 4..8 s; each shows its shape from 1 to 2 s into itself
    let (n1, x1) = orange(&frame(&out, 1.5));
    assert!(n1 > 500, "the first clip's shape is on at 1.5 s ({n1})");
    assert!(x1 < W as f64 * 0.4, "on the left, where it was put ({x1})");
    let (n2, x2) = orange(&frame(&out, 5.5));
    assert!(n2 > 500, "the second clip's shape is on at 5.5 s ({n2})");
    assert!(x2 > W as f64 * 0.6, "on the right ({x2})");
    for t in [0.5, 2.5, 3.5, 4.5, 6.5, 7.5] {
        assert_eq!(orange(&frame(&out, t)).0, 0, "no shape at {t} s");
    }
}

#[tokio::test]
async fn a_clip_without_layers_is_left_alone_between_clips_that_have_them() {
    need_ffmpeg!();
    let tmp = tempfile::tempdir().unwrap();
    let (a, b, c) = (
        source(tmp.path(), "a", false),
        source(tmp.path(), "b", false),
        source(tmp.path(), "c", false),
    );
    let clips = [
        clip(&a, vec![], None),
        clip(&b, vec![shape("dot", 0.5, 0.5)], None),
        clip(&c, vec![], None),
    ];
    let out = render(tmp.path(), "mid", &clips, false, &no_media)
        .await
        .unwrap();
    assert_eq!(orange(&frame(&out, 1.5)).0, 0);
    assert!(orange(&frame(&out, 5.5)).0 > 500);
    assert_eq!(orange(&frame(&out, 9.5)).0, 0);
}

#[tokio::test]
async fn the_sound_is_kept_and_a_silent_source_is_given_silence() {
    need_ffmpeg!();
    let tmp = tempfile::tempdir().unwrap();
    let (loud, quiet) = (
        source(tmp.path(), "loud", true),
        source(tmp.path(), "quiet", false),
    );
    // layers on both: one clip has sound of its own, the other none
    let clips = [
        clip(&loud, vec![shape("dot", 0.5, 0.5)], None),
        clip(&quiet, vec![shape("box", 0.5, 0.5)], None),
    ];
    let out = render(tmp.path(), "sound", &clips, true, &no_media)
        .await
        .unwrap();
    let secs = audio_secs(&out).expect("the sequence has sound");
    assert!(
        secs == 0.0 || (secs > 7.5 && secs < 8.5),
        "the whole sequence long ({secs} s)"
    );
    assert!(orange(&frame(&out, 1.5)).0 > 500);
    assert!(orange(&frame(&out, 5.5)).0 > 500);
}

#[tokio::test]
async fn captions_and_layers_are_drawn_together_with_the_layer_under_the_text() {
    need_ffmpeg!();
    let tmp = tempfile::tempdir().unwrap();
    let a = source(tmp.path(), "a", false);
    let meme = Caption::meme(Some("TOP TEXT".into()), None, 36);
    let clips = [clip(&a, vec![shape("dot", 0.5, 0.5)], Some(meme))];
    let out = render(tmp.path(), "both", &clips, false, &no_media)
        .await
        .unwrap();
    let f = frame(&out, 1.5);
    assert!(orange(&f).0 > 500, "the shape is there");
    assert!(white_on_top(&f) > 100, "and so is the caption text");
}

#[tokio::test]
async fn a_picture_that_is_gone_fails_the_export_and_leaves_nothing_behind() {
    need_ffmpeg!();
    let tmp = tempfile::tempdir().unwrap();
    let (a, b) = (
        source(tmp.path(), "a", false),
        source(tmp.path(), "b", false),
    );
    let mut gone = shape("dot", 0.5, 0.5);
    gone.kind = LayerKind::Media {
        asset: "missing.png".into(),
    };
    let clips = [clip(&a, vec![], None), clip(&b, vec![gone], None)];
    let err = render(tmp.path(), "gone", &clips, false, &no_media)
        .await
        .unwrap_err();
    assert!(format!("{err:#}").contains("no longer there"), "{err:#}");
    assert!(!tmp.path().join("gone.mp4").exists());
    let left: Vec<String> = std::fs::read_dir(tmp.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.contains("job-gone"))
        .collect();
    assert!(left.is_empty(), "temp files left behind: {left:?}");
}

#[test]
fn a_bad_layer_on_one_clip_is_refused_naming_the_clip() {
    let spec = |layers: Vec<Layer>| SequenceClipSpec {
        rating_key: "abc".into(),
        start_ms: 0,
        end_ms: 4000,
        caption: None,
        subtitle_offset_ms: 0,
        layers,
    };
    let mut late = shape("dot", 0.5, 0.5);
    late.start_ms = 9000; // outside a 4 s clip
    assert!(export::validate_sequence(&[spec(vec![]), spec(vec![shape("dot", 0.5, 0.5)])]).is_ok());
    let err = export::validate_sequence(&[spec(vec![]), spec(vec![late])]).unwrap_err();
    assert!(err.contains("abc") && err.contains("#1"), "{err}");
}
