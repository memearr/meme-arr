//! The look of the picture rendered by the real ffmpeg: colour, mirror and fades really change
//! the pixels, and the layers laid on top keep their own colours. Skips when ffmpeg is not installed.

use meme_arr::export::{self, ExportRequest, Format, Grade, Layer, LayerKind, LayerSource, Tone};
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

macro_rules! need_ffmpeg {
    () => {
        if !ffmpeg_available() {
            eprintln!("skipping grade test: ffmpeg not found on PATH");
            return;
        }
    };
}

/// A 4 s clip: the left half red, the right half blue.
fn sample(dir: &Path) -> PathBuf {
    let out = dir.join("halves.mp4");
    let ok = Command::new("ffmpeg")
        .args(["-y", "-v", "error", "-f", "lavfi", "-i"])
        .arg("color=c=0x2040ff:duration=4:size=320x240:rate=10,drawbox=x=0:y=0:w=160:h=240:color=0xff2020:t=fill")
        .args(["-c:v", "libx264", "-pix_fmt", "yuv420p"])
        .arg(&out)
        .status()
        .expect("spawn ffmpeg")
        .success();
    assert!(ok);
    out
}

fn request(format: Format, grade: Grade, layers: Vec<Layer>) -> ExportRequest {
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
        audio: false,
        filename: None,
        credit_source: false,
        layers,
        music: None,
        clip_volume: 1.0,
        clip_curve: vec![],
        mixdown: Default::default(),
        speed: 1.0,
        crop: None,
        grade,
    }
}

async fn render(dir: &Path, src: &Path, name: &str, req: ExportRequest) -> PathBuf {
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
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/MemeTestSans.ttf"
        ),
        &|_: &str| -> Option<LayerSource> { None },
    )
    .await
    .unwrap_or_else(|e| panic!("{name}: {e:#}"));
    out
}

/// The mean colour of the 16x16 square at the centre of (`cx`, `cy`) in the frame at `at_s`.
fn colour(file: &Path, at_s: f64, cx: usize, cy: usize) -> [u8; 3] {
    let out = Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(file)
        .args(["-vf", "fps=10", "-ss", &format!("{at_s}"), "-frames:v", "1"])
        .args(["-f", "rawvideo", "-pix_fmt", "rgb24", "-"])
        .output()
        .expect("spawn ffmpeg");
    assert!(out.status.success());
    assert_eq!(out.stdout.len(), W * H * 3);
    let mut sum = [0u32; 3];
    for y in cy - 8..cy + 8 {
        for x in cx - 8..cx + 8 {
            for (c, total) in sum.iter_mut().enumerate() {
                *total += out.stdout[(y * W + x) * 3 + c] as u32;
            }
        }
    }
    [
        (sum[0] / 256) as u8,
        (sum[1] / 256) as u8,
        (sum[2] / 256) as u8,
    ]
}

fn luma(c: [u8; 3]) -> f32 {
    0.299 * c[0] as f32 + 0.587 * c[1] as f32 + 0.114 * c[2] as f32
}

#[tokio::test]
async fn a_plain_grade_leaves_the_colours_alone() {
    need_ffmpeg!();
    let tmp = tempfile::tempdir().unwrap();
    let src = sample(tmp.path());
    let out = render(
        tmp.path(),
        &src,
        "plain",
        request(Format::Mp4, Grade::default(), vec![]),
    )
    .await;
    let left = colour(&out, 1.0, 80, 120);
    let right = colour(&out, 1.0, 240, 120);
    assert!(left[0] > 180 && left[2] < 90, "red on the left: {left:?}");
    assert!(
        right[2] > 180 && right[0] < 90,
        "blue on the right: {right:?}"
    );
}

#[tokio::test]
async fn black_and_white_takes_the_colour_out_and_sepia_warms_it() {
    need_ffmpeg!();
    let tmp = tempfile::tempdir().unwrap();
    let src = sample(tmp.path());
    let g = Grade {
        tone: Tone::Bw,
        ..Grade::default()
    };
    let bw = render(tmp.path(), &src, "bw", request(Format::Mp4, g, vec![])).await;
    let c = colour(&bw, 1.0, 80, 120);
    assert!(
        c[0].abs_diff(c[1]) < 8 && c[1].abs_diff(c[2]) < 8,
        "grey: {c:?}"
    );

    let g = Grade {
        tone: Tone::Sepia,
        ..Grade::default()
    };
    let sepia = render(tmp.path(), &src, "sepia", request(Format::Mp4, g, vec![])).await;
    let c = colour(&sepia, 1.0, 240, 120);
    assert!(c[0] > c[1] && c[1] > c[2], "brown, not blue: {c:?}");
}

#[tokio::test]
async fn brightness_contrast_and_saturation_change_the_picture() {
    need_ffmpeg!();
    let tmp = tempfile::tempdir().unwrap();
    let src = sample(tmp.path());
    let plain = render(
        tmp.path(),
        &src,
        "p",
        request(Format::Mp4, Grade::default(), vec![]),
    )
    .await;
    let base = luma(colour(&plain, 1.0, 80, 120));

    let g = Grade {
        brightness: 0.3,
        ..Grade::default()
    };
    let bright = render(tmp.path(), &src, "b", request(Format::Mp4, g, vec![])).await;
    assert!(
        luma(colour(&bright, 1.0, 80, 120)) > base + 30.0,
        "brighter"
    );

    let g = Grade {
        saturation: 0.0,
        ..Grade::default()
    };
    let grey = render(tmp.path(), &src, "s", request(Format::Mp4, g, vec![])).await;
    let c = colour(&grey, 1.0, 80, 120);
    assert!(c[0].abs_diff(c[2]) < 8, "no colour left: {c:?}");

    let g = Grade {
        contrast: 0.2,
        ..Grade::default()
    };
    let flat = render(tmp.path(), &src, "c", request(Format::Mp4, g, vec![])).await;
    let (l, r) = (
        luma(colour(&flat, 1.0, 80, 120)),
        luma(colour(&flat, 1.0, 240, 120)),
    );
    let (pl, pr) = (
        luma(colour(&plain, 1.0, 80, 120)),
        luma(colour(&plain, 1.0, 240, 120)),
    );
    assert!(
        (l - r).abs() < (pl - pr).abs(),
        "less contrast between the halves"
    );
}

#[tokio::test]
async fn a_mirror_swaps_left_and_right() {
    need_ffmpeg!();
    let tmp = tempfile::tempdir().unwrap();
    let src = sample(tmp.path());
    let g = Grade {
        mirror: true,
        ..Grade::default()
    };
    let out = render(tmp.path(), &src, "mirror", request(Format::Mp4, g, vec![])).await;
    let left = colour(&out, 1.0, 80, 120);
    assert!(
        left[2] > 180 && left[0] < 90,
        "blue is on the left now: {left:?}"
    );
}

#[tokio::test]
async fn fades_start_and_end_on_black_and_leave_the_middle_alone() {
    need_ffmpeg!();
    let tmp = tempfile::tempdir().unwrap();
    let src = sample(tmp.path());
    let g = Grade {
        fade_in_ms: 1000,
        fade_out_ms: 1000,
        ..Grade::default()
    };
    for format in [Format::Mp4, Format::Gif] {
        let out = render(
            tmp.path(),
            &src,
            &format!("fade-{}", format.ext()),
            request(format, g, vec![]),
        )
        .await;
        let first = luma(colour(&out, 0.0, 80, 120));
        let middle = luma(colour(&out, 2.0, 80, 120));
        let last = luma(colour(&out, 3.8, 80, 120));
        assert!(first < 25.0, "{} starts black: {first}", format.ext());
        assert!(
            middle > 60.0,
            "{} middle is the picture: {middle}",
            format.ext()
        );
        assert!(
            last < 0.4 * middle,
            "{} ends nearly black: {last} vs {middle}",
            format.ext()
        );
    }
}

#[tokio::test]
async fn a_layer_keeps_its_own_colour_over_a_graded_picture_and_does_not_flip() {
    need_ffmpeg!();
    let tmp = tempfile::tempdir().unwrap();
    let src = sample(tmp.path());
    // a green dot at the centre, for the whole clip
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
    let g = Grade {
        tone: Tone::Bw,
        mirror: true,
        ..Grade::default()
    };
    let out = render(
        tmp.path(),
        &src,
        "layered",
        request(Format::Mp4, g, vec![layer]),
    )
    .await;
    let centre = colour(&out, 1.0, 160, 120);
    assert!(
        centre[1] > 150 && centre[0] < 100,
        "the layer is still green: {centre:?}"
    );
    let side = colour(&out, 1.0, 40, 120);
    assert!(
        side[0].abs_diff(side[2]) < 8,
        "the picture around it is grey: {side:?}"
    );
}

#[tokio::test]
async fn a_bad_grade_is_refused() {
    need_ffmpeg!();
    let tmp = tempfile::tempdir().unwrap();
    let src = sample(tmp.path());
    for (name, g) in [
        (
            "bright",
            Grade {
                brightness: 2.0,
                ..Grade::default()
            },
        ),
        (
            "fades",
            Grade {
                fade_in_ms: 3000,
                fade_out_ms: 3000,
                ..Grade::default()
            },
        ),
    ] {
        let out = tmp.path().join(format!("{name}.mp4"));
        let err = export::run_export_with(
            TOOLS,
            export::JobSpec {
                id: "job-bad",
                tmp_dir: tmp.path(),
                out_path: &out,
                source_title: None,
            },
            &src,
            &request(Format::Mp4, g, vec![]),
            &[],
            "/nonexistent/font.ttf",
            &|_: &str| -> Option<LayerSource> { None },
        )
        .await
        .unwrap_err();
        assert!(!err.to_string().is_empty(), "{name}");
    }
}
