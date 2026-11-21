//! Layers rendered by the real ffmpeg: a shape, an uploaded picture and a looping clip
//! really appear where and when they were asked to, over the right picture, in every
//! format, with the sound kept. Skips when ffmpeg is not installed.

use meme_arr::export::{
    self, Caption, ExportRequest, Format, Layer, LayerKind, LayerSource, TextLook,
};
use meme_arr::sprite::{self, Shape};
use meme_arr::subtitles::Cue;
use std::path::{Path, PathBuf};
use std::process::Command;

const TOOLS: export::Tools<'static> = export::Tools {
    ffmpeg: "ffmpeg",
    ffprobe: "ffprobe",
    gifsicle: "gifsicle",
};

const W: usize = 320;
const H: usize = 240;

fn ffmpeg_available() -> bool {
    Command::new("ffmpeg")
        .arg("-version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// A 4 s, 320x240 clip of one flat grey, with a tone when `audio`.
fn sample(dir: &Path, audio: bool) -> PathBuf {
    let out = dir.join(if audio { "flat-audio.mp4" } else { "flat.mp4" });
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

fn shape(name: &str, color: Option<&str>) -> Layer {
    Layer {
        id: "l".into(),
        kind: LayerKind::Shape {
            shape: name.into(),
            color: color.map(String::from),
        },
        start_ms: 1000,
        end_ms: Some(2000),
        x: 0.5,
        y: 0.5,
        size: 0.4,
        rotation: 0.0,
        opacity: 1.0,
        path: vec![],
    }
}

fn request(format: Format, layers: Vec<Layer>, audio: bool) -> ExportRequest {
    ExportRequest {
        rating_key: "t".into(),
        start_ms: 0,
        end_ms: 4000,
        format,
        width: W as u32,
        fps: 10,
        quality: 95,
        caption: None,
        subtitle_offset_ms: 0,
        audio,
        filename: None,
        credit_source: false,
        layers,
        music: None,
        clip_volume: 1.0,
        clip_curve: vec![],
        mixdown: Default::default(),
        speed: 1.0,
        crop: None,
        grade: export::Grade::default(),
    }
}

async fn render(
    dir: &Path,
    src: &Path,
    name: &str,
    req: ExportRequest,
    cues: &[Cue],
    media: &(dyn Fn(&str) -> Option<LayerSource> + Sync),
) -> PathBuf {
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
        cues,
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/MemeTestSans.ttf"
        ),
        media,
    )
    .await
    .unwrap_or_else(|e| panic!("{name}: {e:#}"));
    out
}

fn no_media(_: &str) -> Option<LayerSource> {
    None
}

/// One frame as `W * H * 3` RGB bytes.
///
/// The file is resampled to the exports' 10 fps and sought *after* decoding, not before:
/// an optimised GIF (gifsicle merges identical neighbouring frames into one frame with a
/// long delay) has no frame starting at 1.5 s when the picture was the same from 1.0 s, and
/// seeking before the input would skip the frame that is on screen then for the one after it.
fn rgb_frame(file: &Path, at_s: f64) -> Vec<u8> {
    let out = Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(file)
        .args(["-vf", "fps=10", "-ss", &format!("{at_s}")])
        .args(["-frames:v", "1", "-f", "rawvideo", "-pix_fmt", "rgb24", "-"])
        .output()
        .expect("spawn ffmpeg");
    assert!(out.status.success());
    assert_eq!(out.stdout.len(), W * H * 3, "unexpected frame size");
    out.stdout
}

fn pixel(frame: &[u8], x: usize, y: usize) -> [u8; 3] {
    let i = (y * W + x) * 3;
    [frame[i], frame[i + 1], frame[i + 2]]
}

/// How many pixels are clearly "orange" (the default shape colour), and their mean (x, y).
fn orange(frame: &[u8]) -> (usize, f64, f64) {
    let (mut n, mut sx, mut sy) = (0usize, 0f64, 0f64);
    for y in 0..H {
        for x in 0..W {
            let [r, g, b] = pixel(frame, x, y);
            if r > 200 && (40..130).contains(&g) && b < 60 {
                n += 1;
                sx += x as f64;
                sy += y as f64;
            }
        }
    }
    if n == 0 {
        return (0, 0.0, 0.0);
    }
    (n, sx / n as f64, sy / n as f64)
}

#[tokio::test]
async fn a_shape_appears_at_its_place_and_only_while_it_is_on() {
    if !ffmpeg_available() {
        eprintln!("skipping layers test: ffmpeg not found on PATH");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let src = sample(tmp.path(), false);
    let mut arrow = shape("arrow", None);
    arrow.x = 0.25;
    arrow.y = 0.3;
    let out = render(
        tmp.path(),
        &src,
        "arrow",
        request(Format::Mp4, vec![arrow], false),
        &[],
        &no_media,
    )
    .await;
    // before it starts and after it ends: nothing
    assert_eq!(orange(&rgb_frame(&out, 0.5)).0, 0, "not yet");
    assert_eq!(orange(&rgb_frame(&out, 3.0)).0, 0, "already gone");
    // while it is on: 40% of 320 px wide, centred at (25%, 30%)
    let (n, x, y) = orange(&rgb_frame(&out, 1.5));
    assert!(n > 2000, "the arrow should be there ({n} pixels)");
    assert!((x - 0.25 * W as f64).abs() < 12.0, "mean column {x}");
    assert!((y - 0.3 * H as f64).abs() < 25.0, "mean row {y}");
}

#[tokio::test]
async fn size_turn_colour_and_opacity_all_take_effect() {
    if !ffmpeg_available() {
        eprintln!("skipping layers test: ffmpeg not found on PATH");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let src = sample(tmp.path(), false);

    let small = {
        let mut l = shape("dot", None);
        l.size = 0.1;
        l
    };
    let big = {
        let mut l = shape("dot", None);
        l.size = 0.4;
        l
    };
    let ink = |out: &Path| orange(&rgb_frame(out, 1.5)).0;
    let a = render(
        tmp.path(),
        &src,
        "small",
        request(Format::Mp4, vec![small], false),
        &[],
        &no_media,
    )
    .await;
    let b = render(
        tmp.path(),
        &src,
        "big",
        request(Format::Mp4, vec![big], false),
        &[],
        &no_media,
    )
    .await;
    assert!(ink(&b) > ink(&a) * 8, "{} vs {}", ink(&b), ink(&a));

    // a half-turn points the arrow down: its head (the wide part) is now below the middle
    let mut up = shape("arrow", None);
    up.y = 0.5;
    let mut down = up.clone();
    down.rotation = 180.0;
    let up_out = render(
        tmp.path(),
        &src,
        "up",
        request(Format::Mp4, vec![up], false),
        &[],
        &no_media,
    )
    .await;
    let down_out = render(
        tmp.path(),
        &src,
        "down",
        request(Format::Mp4, vec![down], false),
        &[],
        &no_media,
    )
    .await;
    let (_, _, up_y) = orange(&rgb_frame(&up_out, 1.5));
    let (_, _, down_y) = orange(&rgb_frame(&down_out, 1.5));
    // the upright arrow's mass is in its shaft below the head; turned over it is above it
    assert!(
        up_y > H as f64 * 0.5 && down_y < H as f64 * 0.5,
        "{up_y} / {down_y}"
    );

    // a chosen colour replaces the orange
    let green = shape("dot", Some("#00ff00"));
    let g = render(
        tmp.path(),
        &src,
        "green",
        request(Format::Mp4, vec![green], false),
        &[],
        &no_media,
    )
    .await;
    let [r, gg, bb] = pixel(&rgb_frame(&g, 1.5), W / 2, H / 2);
    assert!(gg > 200 && r < 80 && bb < 80, "{r},{gg},{bb}");

    // half opacity lands about midway between the picture and the colour
    let mut faded = shape("dot", Some("#ffffff"));
    faded.opacity = 0.5;
    let f = render(
        tmp.path(),
        &src,
        "faded",
        request(Format::Mp4, vec![faded], false),
        &[],
        &no_media,
    )
    .await;
    let [r, ..] = pixel(&rgb_frame(&f, 1.5), W / 2, H / 2);
    assert!(
        (120..170).contains(&r),
        "grey 48 under white at 50% is about 151, got {r}"
    );
}

#[tokio::test]
async fn an_uploaded_picture_and_a_looping_clip_are_laid_over_it() {
    if !ffmpeg_available() {
        eprintln!("skipping layers test: ffmpeg not found on PATH");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let src = sample(tmp.path(), false);

    // an "uploaded" picture: the green ring, kept as a file
    let sticker = tmp.path().join("sticker.png");
    std::fs::write(&sticker, sprite::shape_png(Shape::Dot, [0, 255, 0])).unwrap();
    // and a one-second clip of solid red, shorter than the time it is on screen
    let clip = tmp.path().join("red.mp4");
    assert!(Command::new("ffmpeg")
        .args([
            "-y",
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "color=c=red:duration=1:size=64x64:rate=10"
        ])
        .args(["-c:v", "libx264", "-pix_fmt", "yuv420p"])
        .arg(&clip)
        .status()
        .unwrap()
        .success());

    let media = |id: &str| match id {
        "sticker-1.png" => Some(LayerSource {
            path: sticker.clone(),
            motion: false,
        }),
        "red-1.mp4" => Some(LayerSource {
            path: clip.clone(),
            motion: true,
        }),
        _ => None,
    };
    let picture = Layer {
        kind: LayerKind::Media {
            asset: "sticker-1.png".into(),
        },
        x: 0.25,
        y: 0.5,
        size: 0.3,
        start_ms: 0,
        end_ms: None,
        ..shape("arrow", None)
    };
    let video = Layer {
        kind: LayerKind::Media {
            asset: "red-1.mp4".into(),
        },
        x: 0.75,
        y: 0.5,
        size: 0.3,
        start_ms: 500,
        end_ms: Some(3500), // three seconds of a one-second clip
        ..shape("arrow", None)
    };
    let out = render(
        tmp.path(),
        &src,
        "media",
        request(Format::Mp4, vec![picture, video], false),
        &[],
        &media,
    )
    .await;

    let at = |t: f64| rgb_frame(&out, t);
    let green = |f: &[u8]| {
        let [r, g, b] = pixel(f, W / 4, H / 2);
        g > 200 && r < 60 && b < 60
    };
    let red = |f: &[u8]| {
        let [r, g, b] = pixel(f, 3 * W / 4, H / 2);
        r > 200 && g < 60 && b < 60
    };
    // the picture is up for the whole clip
    assert!(green(&at(0.2)) && green(&at(3.8)));
    // the clip is not up before 0.5 s, is up well after its own one second is over,
    // and is gone after 3.5 s
    assert!(!red(&at(0.2)), "before it starts");
    assert!(red(&at(0.8)), "first pass");
    assert!(
        red(&at(2.6)),
        "it loops rather than vanishing when its own length is over"
    );
    assert!(!red(&at(3.8)), "after it ends");

    // a layer naming a picture that is not there is an error that says which layer
    let missing = Layer {
        kind: LayerKind::Media {
            asset: "gone.png".into(),
        },
        ..shape("arrow", None)
    };
    let req = request(Format::Mp4, vec![missing], false);
    let err = export::run_export_with(
        TOOLS,
        export::JobSpec {
            id: "job-missing",
            tmp_dir: tmp.path(),
            out_path: &tmp.path().join("missing.mp4"),
            source_title: None,
        },
        &src,
        &req,
        &[],
        "/nonexistent/font.ttf",
        &media,
    )
    .await
    .unwrap_err();
    assert!(err.to_string().contains("layer 1"), "{err}");
}

#[tokio::test]
async fn every_format_takes_layers_and_the_sound_is_kept() {
    if !ffmpeg_available() {
        eprintln!("skipping layers test: ffmpeg not found on PATH");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let src = sample(tmp.path(), true);
    for format in [Format::Gif, Format::Webp, Format::Mp4, Format::Webm] {
        let name = format!("all-{}", format.ext());
        let keep_audio = format.supports_audio();
        let out = render(
            tmp.path(),
            &src,
            &name,
            request(format, vec![shape("dot", None)], keep_audio),
            &[],
            &no_media,
        )
        .await;
        assert!(out.metadata().unwrap().len() > 500, "{name} is a real file");
        if format == Format::Webp {
            // ffmpeg itself cannot decode an animated WebP, so check it is one
            let bytes = std::fs::read(&out).unwrap();
            assert!(
                bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP",
                "{name}"
            );
            assert!(
                bytes.windows(4).any(|w| w == b"ANIM"),
                "{name} should be animated"
            );
            continue;
        }
        // the picture decodes and carries the shape (a GIF's palette keeps it close to orange)
        let f = rgb_frame(&out, 1.5);
        let [r, g, b] = pixel(&f, W / 2, H / 2);
        assert!(r > 180 && g < 150 && b < 90, "{name}: {r},{g},{b}");
        // sound in the formats that can carry it
        let streams = Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-show_entries",
                "stream=codec_type",
                "-of",
                "csv=p=0",
            ])
            .arg(&out)
            .output()
            .unwrap();
        let kinds = String::from_utf8_lossy(&streams.stdout).to_string();
        assert!(kinds.contains("video"), "{name}: {kinds}");
        assert_eq!(kinds.contains("audio"), keep_audio, "{name}: {kinds}");
    }
}

#[tokio::test]
async fn captions_are_drawn_on_top_of_the_layers() {
    if !ffmpeg_available() {
        eprintln!("skipping layers test: ffmpeg not found on PATH");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let src = sample(tmp.path(), false);
    let mut req = request(Format::Mp4, vec![shape("dot", None)], false);
    req.caption = Some(Caption::Source {
        cues: None,
        font: None,
        font_scale: 100,
        look: Some(TextLook {
            color: Some("#00ff00".into()),
            outline: Some(0.0),
            x: Some(0.5),
            y: Some(0.5),
            preset: Some("block".into()),
            ..Default::default()
        }),
    });
    let cues = [Cue {
        start_ms: 0,
        end_ms: 4000,
        text: "HELLO".into(),
    }];
    let out = render(tmp.path(), &src, "over", req, &cues, &no_media).await;
    // the dot is under the text: both orange and green are in the same frame, and
    // the middle of the picture (where the text is centred) has green on it
    let f = rgb_frame(&out, 1.5);
    assert!(orange(&f).0 > 2000, "the shape is there");
    let greens = (0..H)
        .flat_map(|y| (0..W).map(move |x| (x, y)))
        .filter(|&(x, y)| {
            let [r, g, b] = pixel(&f, x, y);
            g > 200 && r < 80 && b < 80
        })
        .count();
    assert!(
        greens > 300,
        "the text is drawn over the shape ({greens} pixels)"
    );
}

#[tokio::test]
async fn a_layer_moves_along_its_keyframes_and_holds_at_the_last() {
    if !ffmpeg_available() {
        eprintln!("skipping layers test: ffmpeg not found on PATH");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let src = sample(tmp.path(), false);
    let dot = Layer {
        start_ms: 0,
        end_ms: None,
        size: 0.2,
        path: vec![
            export::Keyframe {
                t_ms: 0,
                x: 0.2,
                y: 0.5,
            },
            export::Keyframe {
                t_ms: 2000,
                x: 0.8,
                y: 0.5,
            },
            export::Keyframe {
                t_ms: 3000,
                x: 0.8,
                y: 0.2,
            },
        ],
        ..shape("dot", None)
    };
    let out = render(
        tmp.path(),
        &src,
        "moving",
        request(Format::Mp4, vec![dot], false),
        &[],
        &no_media,
    )
    .await;
    let at = |t: f64| orange(&rgb_frame(&out, t));
    // straight line from 20% to 80% across over two seconds, then held there
    for (t, want_x) in [
        (0.0, 0.2),
        (0.5, 0.35),
        (1.0, 0.5),
        (1.5, 0.65),
        (2.0, 0.8),
        (3.5, 0.8),
    ] {
        let (n, x, _) = at(t);
        assert!(n > 500, "the dot should be there at {t} s ({n} pixels)");
        assert!(
            (x - want_x * W as f64).abs() < 9.0,
            "at {t} s the dot is at column {x}, wanted about {}",
            want_x * W as f64
        );
    }
    // then up from half way to 20% of the height between 2 s and 3 s
    let (_, _, y_mid) = at(2.5);
    let (_, _, y_end) = at(3.5);
    assert!((y_mid - 0.35 * H as f64).abs() < 9.0, "row {y_mid}");
    assert!((y_end - 0.2 * H as f64).abs() < 9.0, "row {y_end}");
}

#[tokio::test]
async fn a_path_with_the_most_keyframes_still_renders() {
    if !ffmpeg_available() {
        eprintln!("skipping layers test: ffmpeg not found on PATH");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let src = sample(tmp.path(), false);
    // a wiggle with the largest number of keyframes a path may have
    let path: Vec<_> = (0..export::MAX_PATH_POINTS as i64)
        .map(|i| export::Keyframe {
            t_ms: i * 4000 / (export::MAX_PATH_POINTS as i64 - 1),
            x: 0.1 + 0.8 * (i as f32 / (export::MAX_PATH_POINTS as f32 - 1.0)),
            y: if i % 2 == 0 { 0.4 } else { 0.6 },
        })
        .collect();
    let dot = Layer {
        start_ms: 0,
        end_ms: None,
        path,
        ..shape("dot", None)
    };
    let out = render(
        tmp.path(),
        &src,
        "wiggle",
        request(Format::Mp4, vec![dot], false),
        &[],
        &no_media,
    )
    .await;
    let (n, x, _) = orange(&rgb_frame(&out, 2.0));
    assert!(n > 500);
    assert!(
        (x - 0.5 * W as f64).abs() < 12.0,
        "half way along at half time, got {x}"
    );
}

#[tokio::test]
async fn a_bad_path_is_refused_before_an_export_starts() {
    let bad = |kf: export::Keyframe| Layer {
        path: vec![kf],
        ..shape("dot", None)
    };
    for (kf, why) in [
        (
            export::Keyframe {
                t_ms: 9000,
                x: 0.5,
                y: 0.5,
            },
            "within the clip",
        ),
        (
            export::Keyframe {
                t_ms: 0,
                x: 2.0,
                y: 0.5,
            },
            "inside the picture",
        ),
    ] {
        let got = export::validate_layers(&[bad(kf)], 4000).unwrap();
        assert!(got.starts_with("layer 1:") && got.contains(why), "{got}");
    }
}

#[tokio::test]
async fn layers_go_with_meme_text_too() {
    if !ffmpeg_available() {
        eprintln!("skipping layers test: ffmpeg not found on PATH");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let src = sample(tmp.path(), false);
    let mut req = request(Format::Mp4, vec![shape("dot", None)], false);
    req.caption = Some(Caption::Meme {
        top: Some("TOP TEXT".into()),
        bottom: Some("BOTTOM TEXT".into()),
        font_size: 36,
        font: None,
        font_scale: 100,
    });
    let out = render(tmp.path(), &src, "meme", req, &[], &no_media).await;
    let f = rgb_frame(&out, 1.5);
    assert!(orange(&f).0 > 2000, "the shape is there under meme text");
}
