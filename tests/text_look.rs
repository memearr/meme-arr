//! A styled caption (colour, place, word-by-word reveal) rendered by the real ffmpeg:
//! the text really lands where it was asked to, and the words really come in one
//! by one. Skips when ffmpeg is not installed, like the other pipeline tests.

use meme_arr::export::{self, Caption, ExportRequest, Format, TextAnim, TextLook, TextReveal};
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

/// A 4 s, 320x240 clip of one flat colour: any text on it is the only thing that changes.
fn flat_sample(dir: &Path) -> PathBuf {
    let out = dir.join("flat.mp4");
    let status = Command::new("ffmpeg")
        .args(["-y", "-v", "error", "-f", "lavfi", "-i"])
        .arg("color=c=0x303030:duration=4:size=320x240:rate=10")
        .args(["-c:v", "libx264", "-pix_fmt", "yuv420p"])
        .arg(&out)
        .status()
        .expect("spawn ffmpeg");
    assert!(status.success());
    out
}

async fn render(dir: &Path, sample: &Path, name: &str, caption: Caption, cues: &[Cue]) -> PathBuf {
    let req = ExportRequest {
        rating_key: "t".into(),
        start_ms: 0,
        end_ms: 4000,
        format: Format::Mp4,
        width: W as u32,
        fps: 10,
        quality: 90,
        caption: Some(caption),
        subtitle_offset_ms: 0,
        audio: false,
        filename: None,
        credit_source: false,
        layers: vec![],
    };
    let out = dir.join(format!("{name}.mp4"));
    export::run_export(
        TOOLS,
        export::JobSpec {
            id: &format!("job-{name}"),
            tmp_dir: dir,
            out_path: &out,
            source_title: None,
        },
        sample,
        &req,
        cues,
        "/nonexistent/font.ttf",
    )
    .await
    .unwrap_or_else(|e| panic!("{name}: {e:#}"));
    out
}

/// One greyscale frame, `W * H` bytes.
fn gray_frame(file: &Path, at_s: f64) -> Vec<u8> {
    let out = Command::new("ffmpeg")
        .args(["-v", "error", "-ss", &format!("{at_s}"), "-i"])
        .arg(file)
        .args(["-frames:v", "1", "-f", "rawvideo", "-pix_fmt", "gray", "-"])
        .output()
        .expect("spawn ffmpeg");
    assert!(out.status.success());
    assert_eq!(out.stdout.len(), W * H, "unexpected frame size");
    out.stdout
}

/// Where two frames differ: how many pixels, and the mean (x, y) of those pixels.
fn difference(a: &[u8], b: &[u8]) -> (usize, f64, f64) {
    let (mut n, mut sx, mut sy) = (0usize, 0f64, 0f64);
    for (i, (p, q)) in a.iter().zip(b).enumerate() {
        if p.abs_diff(*q) > 40 {
            n += 1;
            sx += (i % W) as f64;
            sy += (i / W) as f64;
        }
    }
    if n == 0 {
        return (0, 0.0, 0.0);
    }
    (n, sx / n as f64, sy / n as f64)
}

fn look_caption(look: TextLook) -> Caption {
    Caption::Source {
        cues: None,
        font: None,
        font_scale: 100,
        look: Some(look),
    }
}

fn line() -> Vec<Cue> {
    vec![Cue {
        start_ms: 0,
        end_ms: 4000,
        text: "WORDS COME IN ONE BY ONE HERE".into(),
    }]
}

#[tokio::test]
async fn text_lands_where_it_is_placed_and_the_size_slider_changes_its_size() {
    if !ffmpeg_available() {
        eprintln!("skipping text look test: ffmpeg not found on PATH");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let sample = flat_sample(tmp.path());
    let plain = render(tmp.path(), &sample, "plain", Caption::None, &[]).await;
    let base = gray_frame(&plain, 3.0);

    // the default: resting on the bottom, centred
    let bottom = render(
        tmp.path(),
        &sample,
        "bottom",
        look_caption(TextLook::default()),
        &line(),
    )
    .await;
    let (n, x, y) = difference(&base, &gray_frame(&bottom, 3.0));
    assert!(n > 200, "the text should be drawn ({n} pixels)");
    assert!(y > H as f64 * 0.8, "bottom text sits low, mean row {y}");
    assert!(
        (x - W as f64 / 2.0).abs() < 40.0,
        "centred, mean column {x}"
    );

    // placed in the upper left
    let corner = render(
        tmp.path(),
        &sample,
        "corner",
        look_caption(TextLook {
            x: Some(0.3),
            y: Some(0.15),
            ..Default::default()
        }),
        &[Cue {
            start_ms: 0,
            end_ms: 4000,
            text: "HI".into(),
        }],
    )
    .await;
    let (n, x, y) = difference(&base, &gray_frame(&corner, 3.0));
    assert!(n > 50, "the text should be drawn ({n} pixels)");
    assert!(y < H as f64 * 0.3, "high up, mean row {y}");
    assert!(x < W as f64 * 0.45, "left of centre, mean column {x}");

    // a bigger size slider means more ink
    let big = render(
        tmp.path(),
        &sample,
        "big",
        Caption::Source {
            cues: None,
            font: None,
            font_scale: 200,
            look: Some(TextLook::default()),
        },
        &[Cue {
            start_ms: 0,
            end_ms: 4000,
            text: "HI".into(),
        }],
    )
    .await;
    let small = render(
        tmp.path(),
        &sample,
        "small",
        look_caption(TextLook::default()),
        &[Cue {
            start_ms: 0,
            end_ms: 4000,
            text: "HI".into(),
        }],
    )
    .await;
    let ink = |f: &Path| difference(&base, &gray_frame(f, 3.0)).0;
    assert!(
        ink(&big) > ink(&small) * 2,
        "{} vs {}",
        ink(&big),
        ink(&small)
    );
}

#[tokio::test]
async fn words_come_in_one_by_one_and_a_still_shows_them_all() {
    if !ffmpeg_available() {
        eprintln!("skipping text look test: ffmpeg not found on PATH");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let sample = flat_sample(tmp.path());
    let plain = render(tmp.path(), &sample, "plain", Caption::None, &[]).await;
    let revealed = render(
        tmp.path(),
        &sample,
        "reveal",
        look_caption(TextLook {
            reveal: TextReveal::Word,
            anim: TextAnim::Fade,
            ..Default::default()
        }),
        &line(),
    )
    .await;
    let ink = |t: f64| difference(&gray_frame(&plain, t), &gray_frame(&revealed, t)).0;
    let (early, middle, late) = (ink(0.1), ink(1.6), ink(3.5));
    assert!(early > 0, "the first word is up at once");
    assert!(
        early < middle && middle < late,
        "words should accumulate: {early} < {middle} < {late}"
    );
    // by the end every word is up: about as much text as with no reveal at all
    let whole = render(
        tmp.path(),
        &sample,
        "whole",
        look_caption(TextLook::default()),
        &line(),
    )
    .await;
    let full = difference(&gray_frame(&plain, 3.5), &gray_frame(&whole, 3.5)).0;
    assert!(
        late as f64 > full as f64 * 0.9,
        "revealed {late} vs whole {full}"
    );
}

#[tokio::test]
async fn text_follows_its_keyframes_across_the_picture() {
    if !ffmpeg_available() {
        eprintln!("skipping text look test: ffmpeg not found on PATH");
        return;
    }
    use meme_arr::export::Keyframe;
    let tmp = tempfile::tempdir().unwrap();
    let sample = flat_sample(tmp.path());
    let plain = render(tmp.path(), &sample, "plain", Caption::None, &[]).await;
    let moving = render(
        tmp.path(),
        &sample,
        "moving",
        look_caption(TextLook {
            preset: Some("block".into()),
            path: vec![
                Keyframe {
                    t_ms: 0,
                    x: 0.2,
                    y: 0.3,
                },
                Keyframe {
                    t_ms: 3000,
                    x: 0.8,
                    y: 0.7,
                },
            ],
            ..Default::default()
        }),
        &[Cue {
            start_ms: 0,
            end_ms: 4000,
            text: "GO".into(),
        }],
    )
    .await;
    // where the text is at each moment: its mean column and row
    let at = |t: f64| {
        let (n, x, y) = difference(&gray_frame(&plain, t), &gray_frame(&moving, t));
        assert!(n > 100, "the text should be there at {t} s ({n} pixels)");
        (x, y)
    };
    for (t, fx, fy) in [
        (0.1, 0.2, 0.3),
        (1.5, 0.5, 0.5),
        (3.0, 0.8, 0.7),
        (3.8, 0.8, 0.7),
    ] {
        let (x, y) = at(t);
        assert!(
            (x - fx * W as f64).abs() < 16.0 && (y - fy * H as f64).abs() < 16.0,
            "at {t} s the text is at ({x:.0}, {y:.0}), wanted about ({:.0}, {:.0})",
            fx * W as f64,
            fy * H as f64
        );
    }
}
