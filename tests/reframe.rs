//! Speed, reframing and a sequence clip's picture look, rendered by the real ffmpeg. Skips when
//! ffmpeg is not installed.

use meme_arr::export::{
    self, Crop, ExportRequest, Format, Grade, Layer, LayerKind, LayerSource, ResolvedSequenceClip,
    SequenceExportRequest, Tone,
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

macro_rules! need_ffmpeg {
    () => {
        if !Command::new("ffmpeg")
            .arg("-version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
        {
            eprintln!("skipping reframe test: ffmpeg not found on PATH");
            return;
        }
    };
}

/// A 4 s clip, 320x240: the left half red, the right half blue, with a 440 Hz tone.
fn sample(dir: &Path) -> PathBuf {
    let out = dir.join("halves.mp4");
    let ok = Command::new("ffmpeg")
        .args(["-y", "-v", "error", "-f", "lavfi", "-i"])
        .arg("color=c=0x2040ff:duration=4:size=320x240:rate=10,drawbox=x=0:y=0:w=160:h=240:color=0xff2020:t=fill")
        .args(["-f", "lavfi", "-i", "sine=frequency=440:duration=4"])
        .args(["-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac", "-shortest"])
        .arg(&out)
        .status()
        .expect("spawn ffmpeg")
        .success();
    assert!(ok);
    out
}

fn request(format: Format, audio: bool) -> ExportRequest {
    ExportRequest {
        rating_key: "t".into(),
        start_ms: 0,
        end_ms: 4000,
        format,
        width: 320,
        fps: 10,
        quality: 90,
        caption: None,
        subtitle_offset_ms: 0,
        audio,
        filename: None,
        credit_source: false,
        layers: vec![],
        music: None,
        clip_volume: 1.0,
        clip_curve: vec![],
        mixdown: Default::default(),
        speed: 1.0,
        crop: None,
        grade: Grade::default(),
    }
}

async fn try_render(
    dir: &Path,
    src: &Path,
    name: &str,
    req: ExportRequest,
) -> anyhow::Result<PathBuf> {
    let out = dir.join(format!("{name}.{}", req.format.ext()));
    export::run_export_with(
        TOOLS,
        export::JobSpec {
            id: &format!("job-{name}"),
            tmp_dir: dir,
            out_path: &out,
            source_title: None,
        },
        src,
        &req,
        &[],
        FONT,
        &|_: &str| -> Option<LayerSource> { None },
    )
    .await?;
    Ok(out)
}

async fn render(dir: &Path, src: &Path, name: &str, req: ExportRequest) -> PathBuf {
    try_render(dir, src, name, req)
        .await
        .unwrap_or_else(|e| panic!("{name}: {e:#}"))
}

/// One number from ffprobe: `entries` like "format=duration" or "stream=width", for the stream `sel`.
fn probe(file: &Path, sel: &str, entries: &str) -> f64 {
    let out = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            sel,
            "-show_entries",
            entries,
        ])
        .args(["-of", "default=nw=1:nk=1"])
        .arg(file)
        .output()
        .expect("spawn ffprobe");
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .next()
        .and_then(|l| l.trim().parse().ok())
        .unwrap_or(0.0)
}

fn video_secs(file: &Path) -> f64 {
    // the container's length: the picture and the sound are cut together
    probe(file, "v:0", "format=duration")
}

fn audio_secs(file: &Path) -> f64 {
    let out = Command::new("ffmpeg")
        .args(["-v", "info", "-i"])
        .arg(file)
        .args(["-vn", "-f", "null", "-"])
        .output()
        .expect("spawn ffmpeg");
    let text = String::from_utf8_lossy(&out.stderr).to_string();
    // "time=00:00:02.00" on the last progress line
    text.rsplit("time=")
        .next()
        .and_then(|t| t.split_whitespace().next())
        .map(|t| {
            let p: Vec<f64> = t.split(':').filter_map(|v| v.parse().ok()).collect();
            if p.len() == 3 {
                p[0] * 3600.0 + p[1] * 60.0 + p[2]
            } else {
                0.0
            }
        })
        .unwrap_or(0.0)
}

fn size(file: &Path) -> (u32, u32) {
    (
        probe(file, "v:0", "stream=width") as u32,
        probe(file, "v:0", "stream=height") as u32,
    )
}

/// The mean colour of the whole first frame of the file.
fn mean_colour(file: &Path) -> [u8; 3] {
    let out = Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(file)
        .args(["-vf", "scale=1:1:flags=area", "-frames:v", "1"])
        .args(["-f", "rawvideo", "-pix_fmt", "rgb24", "-"])
        .output()
        .expect("spawn ffmpeg");
    assert_eq!(out.stdout.len(), 3, "one pixel");
    [out.stdout[0], out.stdout[1], out.stdout[2]]
}

#[tokio::test]
async fn a_faster_clip_is_shorter_and_a_slower_one_longer_picture_and_sound_together() {
    need_ffmpeg!();
    let tmp = tempfile::tempdir().unwrap();
    let src = sample(tmp.path());
    for (name, speed, want) in [
        ("fast", 2.0f32, 2.0f64),
        ("slow", 0.5, 8.0),
        ("quad", 4.0, 1.0),
    ] {
        let mut req = request(Format::Mp4, true);
        req.speed = speed;
        let out = render(tmp.path(), &src, name, req).await;
        let (v, a) = (video_secs(&out), audio_secs(&out));
        assert!(
            (v - want).abs() < 0.35,
            "{name}: picture {v} s, wanted about {want}"
        );
        assert!(
            (a - want).abs() < 0.35,
            "{name}: sound {a} s, wanted about {want}"
        );
    }
    // normal speed changes nothing
    let plain = render(tmp.path(), &src, "plain", request(Format::Mp4, true)).await;
    assert!((video_secs(&plain) - 4.0).abs() < 0.2);
}

#[tokio::test]
async fn speed_works_with_layers_a_gif_and_a_song_in_the_mix() {
    need_ffmpeg!();
    let tmp = tempfile::tempdir().unwrap();
    let src = sample(tmp.path());
    let layer = Layer {
        id: "l".into(),
        kind: LayerKind::Shape {
            shape: "dot".into(),
            color: None,
        },
        start_ms: 0,
        end_ms: None,
        x: 0.5,
        y: 0.5,
        size: 0.2,
        rotation: 0.0,
        opacity: 1.0,
        path: vec![],
    };
    let mut req = request(Format::Mp4, true);
    req.speed = 2.0;
    req.layers = vec![layer.clone()];
    let out = render(tmp.path(), &src, "layered", req).await;
    assert!((video_secs(&out) - 2.0).abs() < 0.35);
    assert!((audio_secs(&out) - 2.0).abs() < 0.35);

    let mut req = request(Format::Gif, false);
    req.speed = 2.0;
    let gif = render(tmp.path(), &src, "gif", req).await;
    assert!(
        (video_secs(&gif) - 2.0).abs() < 0.35,
        "{}",
        video_secs(&gif)
    );

    // a volume change makes a mix; the speed is still applied to it
    let mut req = request(Format::Mp4, true);
    req.speed = 2.0;
    req.clip_volume = 0.5;
    let mixed = render(tmp.path(), &src, "mixed", req).await;
    assert!((audio_secs(&mixed) - 2.0).abs() < 0.35);
}

#[tokio::test]
async fn a_crop_keeps_the_window_at_the_width_asked_for() {
    need_ffmpeg!();
    let tmp = tempfile::tempdir().unwrap();
    let src = sample(tmp.path());
    let crop = |x, w| Crop {
        x,
        y: 0.0,
        w,
        h: 1.0,
    };
    let mut req = request(Format::Mp4, false);
    req.crop = Some(crop(0.5, 0.5));
    let right = render(tmp.path(), &src, "right", req).await;
    let (w, h) = size(&right);
    assert_eq!(w, 320, "the width asked for");
    // the right half of a 4:3 picture is 2:3: taller than wide
    assert!(h > w && (h as f64 / w as f64 - 1.5).abs() < 0.05, "{w}x{h}");
    let c = mean_colour(&right);
    assert!(c[2] > 150 && c[0] < 100, "the right half is blue: {c:?}");

    let mut req = request(Format::Mp4, false);
    req.crop = Some(crop(0.0, 0.5));
    let left = render(tmp.path(), &src, "left", req).await;
    let c = mean_colour(&left);
    assert!(c[0] > 150 && c[2] < 100, "the left half is red: {c:?}");

    // a portrait 9:16 window comes out even-sized and close to 9:16
    let mut req = request(Format::Mp4, false);
    req.crop = Some(Crop {
        x: 0.25,
        y: 0.0,
        w: 0.4219,
        h: 1.0,
    });
    let tall = render(tmp.path(), &src, "tall", req).await;
    let (w, h) = size(&tall);
    assert_eq!((w % 2, h % 2), (0, 0));
    assert!(
        (w as f64 / h as f64 - 0.5625 * 0.75 / 0.75).abs() < 0.2,
        "{w}x{h}"
    );
}

#[tokio::test]
async fn a_crop_and_a_speed_go_with_a_layer_and_a_gif() {
    need_ffmpeg!();
    let tmp = tempfile::tempdir().unwrap();
    let src = sample(tmp.path());
    let layer = Layer {
        id: "l".into(),
        kind: LayerKind::Shape {
            shape: "dot".into(),
            color: Some("#00ff00".into()),
        },
        start_ms: 0,
        end_ms: None,
        x: 0.75,
        y: 0.5,
        size: 0.2,
        rotation: 0.0,
        opacity: 1.0,
        path: vec![],
    };
    let mut req = request(Format::Mp4, false);
    req.layers = vec![layer];
    req.crop = Some(Crop {
        x: 0.5,
        y: 0.0,
        w: 0.5,
        h: 1.0,
    });
    req.speed = 2.0;
    let out = render(tmp.path(), &src, "both", req).await;
    assert_eq!(size(&out).0, 320);
    assert!((video_secs(&out) - 2.0).abs() < 0.35);
    // the dot sat at three quarters across: in the cropped window it is the middle of it
    let c = mean_colour(&out);
    assert!(c[1] > 40, "the green dot is in the window: {c:?}");

    let mut req = request(Format::Gif, false);
    req.crop = Some(Crop {
        x: 0.0,
        y: 0.0,
        w: 0.5,
        h: 0.5,
    });
    let gif = render(tmp.path(), &src, "gifcrop", req).await;
    assert_eq!(size(&gif).0, 320);
}

#[tokio::test]
async fn bad_speeds_and_crops_are_refused() {
    need_ffmpeg!();
    let tmp = tempfile::tempdir().unwrap();
    let src = sample(tmp.path());
    let mut req = request(Format::Mp4, false);
    req.speed = 9.0;
    assert!(try_render(tmp.path(), &src, "fast", req).await.is_err());
    let mut req = request(Format::Mp4, false);
    req.crop = Some(Crop {
        x: 0.8,
        y: 0.0,
        w: 0.5,
        h: 1.0,
    });
    assert!(try_render(tmp.path(), &src, "out", req).await.is_err());
}

fn seq_request(audio: bool) -> SequenceExportRequest {
    SequenceExportRequest {
        clips: vec![],
        format: Format::Mp4,
        width: 320,
        fps: 10,
        quality: 90,
        audio,
        filename: None,
        credit_source: false,
        music: None,
        clip_volume: 1.0,
    }
}

fn seq_clip(file: &Path, grade: Grade, layers: Vec<Layer>) -> ResolvedSequenceClip {
    ResolvedSequenceClip {
        file: file.to_path_buf(),
        start_ms: 0,
        end_ms: 4000,
        caption: None,
        subtitle_offset_ms: 0,
        cues: vec![],
        source: String::new(),
        font_path: None,
        layers,
        music: None,
        grade,
    }
}

async fn seq_render(dir: &Path, name: &str, clip: ResolvedSequenceClip, audio: bool) -> PathBuf {
    let out = dir.join(format!("{name}.mp4"));
    export::run_sequence_export_with(
        TOOLS,
        export::JobSpec {
            id: &format!("job-{name}"),
            tmp_dir: dir,
            out_path: &out,
            source_title: None,
        },
        &seq_request(audio),
        &[clip],
        FONT,
        &|_: &str| -> Option<LayerSource> { None },
    )
    .await
    .unwrap_or_else(|e| panic!("{name}: {e:#}"));
    out
}

#[tokio::test]
async fn a_sequence_clip_can_have_its_own_picture_look() {
    need_ffmpeg!();
    let tmp = tempfile::tempdir().unwrap();
    let src = sample(tmp.path());
    // the 4:3 clip is fitted in the 16:9 canvas, so the middle of the frame is the picture and the
    // sides are black bars: the look is on the picture, not the bars
    let plain = seq_render(
        tmp.path(),
        "plain",
        seq_clip(&src, Grade::default(), vec![]),
        false,
    )
    .await;
    let grey = Grade {
        tone: Tone::Bw,
        ..Grade::default()
    };
    let bw = seq_render(tmp.path(), "bw", seq_clip(&src, grey, vec![]), false).await;
    let (p, g) = (mean_colour(&plain), mean_colour(&bw));
    assert!(p[0].abs_diff(p[1]) > 50, "plain is coloured: {p:?}");
    assert!(
        g[0].abs_diff(g[2]) < 8 && g[0].abs_diff(g[1]) < 8,
        "black and white is grey: {g:?}"
    );

    // a fade in starts the clip from black, with the sound and the layers intact
    let faded = Grade {
        fade_in_ms: 1500,
        ..Grade::default()
    };
    let out = seq_render(tmp.path(), "fade", seq_clip(&src, faded, vec![]), true).await;
    let first = mean_colour(&out);
    assert!(first.iter().all(|&c| c < 30), "starts on black: {first:?}");

    // layers and a grade together: the layer keeps its colour
    let layer = Layer {
        id: "l".into(),
        kind: LayerKind::Shape {
            shape: "dot".into(),
            color: Some("#00ff00".into()),
        },
        start_ms: 0,
        end_ms: None,
        x: 0.5,
        y: 0.5,
        size: 0.3,
        rotation: 0.0,
        opacity: 1.0,
        path: vec![],
    };
    let both = seq_render(tmp.path(), "both", seq_clip(&src, grey, vec![layer]), false).await;
    let c = mean_colour(&both);
    assert!(
        c[1] > c[0] && c[1] > c[2],
        "the green dot shows over the grey: {c:?}"
    );
}

#[tokio::test]
async fn a_bad_picture_look_on_a_sequence_clip_is_refused() {
    let mut spec = export::SequenceClipSpec {
        rating_key: "k".into(),
        start_ms: 0,
        end_ms: 2000,
        caption: None,
        subtitle_offset_ms: 0,
        layers: vec![],
        music: None,
        grade: Grade {
            fade_in_ms: 3000,
            ..Grade::default()
        },
    };
    let err = export::validate_sequence(std::slice::from_ref(&spec)).unwrap_err();
    assert!(err.contains("fades"), "{err}");
    spec.grade = Grade::default();
    assert!(export::validate_sequence(&[spec]).is_ok());
}
