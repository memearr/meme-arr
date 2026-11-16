//! The tracker on a real decoded clip: a small textured object moves across a busy
//! picture on a path we know exactly, and the path the tracker returns matches it. Skips
//! when ffmpeg is not installed.

use meme_arr::export::position_at;
use meme_arr::tracker::{track_clip, Refused, TrackRequest};
use std::path::{Path, PathBuf};
use std::process::Command;

const W: f32 = 480.0;
const H: f32 = 270.0;

fn ffmpeg_available() -> bool {
    Command::new("ffmpeg")
        .arg("-version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// A 4 s clip: a busy test pattern, and a 64 px colour-bars square that moves from the left
/// with a wobble. Its centre is at `(72 + 90 t, 132 + 40 sin 2t)`. With `vanish`, the
/// square is gone from 2 s on.
fn moving_object(dir: &Path, vanish: bool) -> PathBuf {
    let out = dir.join(if vanish { "vanish.mp4" } else { "moving.mp4" });
    let enable = if vanish { ":enable='lt(t,2)'" } else { "" };
    // The busy background is a test pattern (which has colour bars of its own, that a
    // vanished square could be mistaken for); the one for the vanishing test is noise.
    let bg = if vanish {
        "nullsrc=size=480x270:rate=24:duration=4,geq=random(1)*255:128:128"
    } else {
        "testsrc=size=480x270:rate=24:duration=4"
    };
    let graph = format!(
        "{bg}[bg];\
         smptebars=size=64x64:rate=24:duration=4[fg];\
         [bg][fg]overlay=x='40+90*t':y='100+40*sin(t*2)'{enable}"
    );
    let status = Command::new("ffmpeg")
        .args(["-y", "-v", "error", "-filter_complex", &graph])
        .args(["-c:v", "libx264", "-pix_fmt", "yuv420p"])
        .arg(&out)
        .status()
        .expect("spawn ffmpeg");
    assert!(status.success());
    out
}

fn truth(t_s: f32) -> (f32, f32) {
    (
        (72.0 + 90.0 * t_s) / W,
        (132.0 + 40.0 * (2.0 * t_s).sin()) / H,
    )
}

fn request(t_s: f32) -> TrackRequest {
    let (x, y) = truth(t_s);
    TrackRequest {
        t0_ms: 0,
        x,
        y,
        size: 64.0 / W,
        max_points: 100,
    }
}

#[tokio::test]
async fn a_moving_object_is_followed_along_its_true_path() {
    if !ffmpeg_available() {
        eprintln!("skipping tracking test: ffmpeg not found on PATH");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let file = moving_object(tmp.path(), false);
    let out = track_clip(("ffmpeg", "ffprobe"), &file, (0, 4000), &request(0.0))
        .await
        .unwrap();
    assert!(!out.lost, "lost it after {} ms", out.tracked_ms);
    assert!(out.tracked_ms >= 3500, "followed for {} ms", out.tracked_ms);
    assert!(
        out.path.len() >= 3 && out.path.len() < 60,
        "{} keyframes describe the wobble",
        out.path.len()
    );
    // the path agrees with where the square really was, to a couple of percent of the
    // picture, all along
    let mut worst = 0f32;
    for ms in (0..=3500).step_by(125) {
        let (x, y) = position_at(&out.path, ms, (0.0, 0.0));
        let (tx, ty) = truth(ms as f32 / 1000.0);
        worst = worst.max((x - tx).hypot(y - ty));
    }
    assert!(worst < 0.02, "worst miss {worst} of the picture");
}

#[tokio::test]
async fn it_follows_from_a_later_moment_of_a_clip_cut_from_the_middle() {
    if !ffmpeg_available() {
        eprintln!("skipping tracking test: ffmpeg not found on PATH");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let file = moving_object(tmp.path(), false);
    // the clip is 1.0 s to 4.0 s of the file; follow from 0.5 s into it (1.5 s of the file)
    let mut req = request(1.5);
    req.t0_ms = 500;
    let out = track_clip(("ffmpeg", "ffprobe"), &file, (1000, 4000), &req)
        .await
        .unwrap();
    assert!(!out.lost);
    // keyframes are in the clip's own time and start at the moment asked for (to a frame)
    assert!((out.path[0].t_ms - 500).abs() <= 85, "{:?}", out.path[0]);
    for clip_ms in (600..=2900).step_by(150) {
        let (x, y) = position_at(&out.path, clip_ms, (0.0, 0.0));
        let (tx, ty) = truth((clip_ms + 1000) as f32 / 1000.0);
        assert!(
            (x - tx).hypot(y - ty) < 0.02,
            "at {clip_ms} ms: ({x},{y}) vs ({tx},{ty})"
        );
    }
}

#[tokio::test]
async fn it_says_when_the_object_goes_away() {
    if !ffmpeg_available() {
        eprintln!("skipping tracking test: ffmpeg not found on PATH");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let file = moving_object(tmp.path(), true);
    let out = track_clip(("ffmpeg", "ffprobe"), &file, (0, 4000), &request(0.0))
        .await
        .unwrap();
    assert!(out.lost, "it should notice the square is gone");
    assert!(
        (1500..=2600).contains(&out.tracked_ms),
        "followed for {} ms; the square is there for 2 s",
        out.tracked_ms
    );
    // what was followed is still a usable path
    assert!(out.path.len() >= 2);
}

#[tokio::test]
async fn a_spot_with_nothing_in_it_is_refused_with_a_reason() {
    if !ffmpeg_available() {
        eprintln!("skipping tracking test: ffmpeg not found on PATH");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let flat = tmp.path().join("flat.mp4");
    assert!(Command::new("ffmpeg")
        .args([
            "-y",
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "color=c=gray:s=320x180:d=2:r=24"
        ])
        .args(["-c:v", "libx264", "-pix_fmt", "yuv420p"])
        .arg(&flat)
        .status()
        .unwrap()
        .success());
    let err = track_clip(
        ("ffmpeg", "ffprobe"),
        &flat,
        (0, 2000),
        &TrackRequest {
            t0_ms: 0,
            x: 0.5,
            y: 0.5,
            size: 0.12,
            max_points: 100,
        },
    )
    .await
    .unwrap_err();
    let refused = err
        .downcast_ref::<Refused>()
        .expect("a reason the caller can act on");
    assert!(refused.0.contains("nothing to follow"), "{refused}");
}

#[tokio::test]
async fn a_clip_that_is_too_long_or_backwards_is_refused() {
    let any = Path::new("/nonexistent.mp4");
    let req = request(0.0);
    for (clip, why) in [((5000, 5000), "after start"), ((0, 61_000), "60 seconds")] {
        let err = track_clip(("ffmpeg", "ffprobe"), any, clip, &req)
            .await
            .unwrap_err();
        let refused = err.downcast_ref::<Refused>().expect("refused, not failed");
        assert!(refused.0.contains(why), "{refused}");
    }
}
