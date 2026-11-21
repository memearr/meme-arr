//! Transitions between the clips of a sequence, rendered by the real ffmpeg: the clips overlap by the
//! transition (so the sequence is shorter), the picture and the sound really blend, every kind works, and
//! a clip's own song starts where its clip does. Skips when ffmpeg is not installed.

use meme_arr::export::{
    self, Format, LayerSource, ResolvedSequenceClip, SequenceClipSpec, SequenceExportRequest,
    Soundtrack, Transition, TransitionKind,
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
            eprintln!("skipping transitions test: ffmpeg not found on PATH");
            return;
        }
    };
}

/// A 3 s clip of one colour with a 440 Hz tone.
fn colour_clip(dir: &Path, name: &str, hex: &str) -> PathBuf {
    let out = dir.join(format!("{name}.mp4"));
    let ok = Command::new("ffmpeg")
        .args(["-y", "-v", "error", "-f", "lavfi", "-i"])
        .arg(format!("color=c={hex}:duration=3:size=320x240:rate=10"))
        .args(["-f", "lavfi", "-i", "sine=frequency=440:duration=3"])
        .args([
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-shortest",
        ])
        .arg(&out)
        .status()
        .expect("spawn ffmpeg")
        .success();
    assert!(ok);
    out
}

fn song_file(dir: &Path) -> PathBuf {
    let out = dir.join("song.wav");
    let ok = Command::new("ffmpeg")
        .args([
            "-y",
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=880:duration=20",
        ])
        .arg(&out)
        .status()
        .expect("spawn ffmpeg")
        .success();
    assert!(ok);
    out
}

fn clip(file: &Path, transition: Option<Transition>) -> ResolvedSequenceClip {
    ResolvedSequenceClip {
        file: file.to_path_buf(),
        start_ms: 0,
        end_ms: 3000,
        caption: None,
        subtitle_offset_ms: 0,
        cues: vec![],
        source: String::new(),
        font_path: None,
        layers: vec![],
        music: None,
        grade: Default::default(),
        transition,
    }
}

fn fade(ms: i64) -> Option<Transition> {
    Some(Transition {
        kind: TransitionKind::Fade,
        ms,
    })
}

fn request(audio: bool) -> SequenceExportRequest {
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

async fn try_render(
    dir: &Path,
    name: &str,
    clips: &[ResolvedSequenceClip],
    audio: bool,
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
        &|_: &str| -> Option<LayerSource> { None },
    )
    .await?;
    Ok(out)
}

async fn render(dir: &Path, name: &str, clips: &[ResolvedSequenceClip], audio: bool) -> PathBuf {
    try_render(dir, name, clips, audio)
        .await
        .unwrap_or_else(|e| panic!("{name}: {e:#}"))
}

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

/// How long the picture runs.
fn secs(file: &Path) -> f64 {
    probe(file, "v:0", "format=duration")
}

fn has_sound(file: &Path) -> bool {
    probe(file, "a:0", "stream=channels") > 0.0
}

/// The mean colour of the frame at `at_s`.
fn colour_at(file: &Path, at_s: f64) -> [u8; 3] {
    let out = Command::new("ffmpeg")
        .args(["-v", "error", "-ss", &format!("{at_s}"), "-i"])
        .arg(file)
        .args(["-vf", "scale=1:1:flags=area", "-frames:v", "1"])
        .args(["-f", "rawvideo", "-pix_fmt", "rgb24", "-"])
        .output()
        .expect("spawn ffmpeg");
    assert_eq!(out.stdout.len(), 3, "a frame at {at_s}");
    [out.stdout[0], out.stdout[1], out.stdout[2]]
}

/// The mean volume (dB) of `len` seconds of the sound from `from`.
fn level(file: &Path, from: f64, len: f64) -> f64 {
    let out = Command::new("ffmpeg")
        .args([
            "-v",
            "info",
            "-ss",
            &from.to_string(),
            "-t",
            &len.to_string(),
            "-i",
        ])
        .arg(file)
        .args(["-vn", "-af", "volumedetect", "-f", "null", "-"])
        .output()
        .expect("spawn ffmpeg");
    String::from_utf8_lossy(&out.stderr)
        .lines()
        .find_map(|l| l.split("mean_volume:").nth(1))
        .and_then(|v| v.trim().trim_end_matches(" dB").parse().ok())
        .unwrap_or(-120.0)
}

#[tokio::test]
async fn a_fade_overlaps_the_clips_and_really_blends_picture_and_sound() {
    need_ffmpeg!();
    let tmp = tempfile::tempdir().unwrap();
    let red = colour_clip(tmp.path(), "red", "0xff1010");
    let blue = colour_clip(tmp.path(), "blue", "0x1010ff");
    let out = render(
        tmp.path(),
        "fade",
        &[clip(&red, None), clip(&blue, fade(1000))],
        true,
    )
    .await;
    // 3 s + 3 s, less the 1 s they share
    let len = secs(&out);
    assert!((len - 5.0).abs() < 0.4, "{len} s");
    assert!(has_sound(&out), "the sound is kept");
    // red, then a blend of both, then blue
    let (a, b, c) = (
        colour_at(&out, 0.5),
        colour_at(&out, 2.5),
        colour_at(&out, 4.5),
    );
    assert!(a[0] > 180 && a[2] < 70, "red first: {a:?}");
    assert!(
        b[0] > 70 && b[2] > 70,
        "both in the middle of the fade: {b:?}"
    );
    assert!(c[2] > 180 && c[0] < 70, "blue last: {c:?}");
    // the sound goes all the way through the transition (a cross-fade, no gap)
    assert!(level(&out, 2.2, 0.6) > -30.0, "{}", level(&out, 2.2, 0.6));
}

#[tokio::test]
async fn cuts_and_transitions_mix_in_one_sequence() {
    need_ffmpeg!();
    let tmp = tempfile::tempdir().unwrap();
    let red = colour_clip(tmp.path(), "red", "0xff1010");
    let blue = colour_clip(tmp.path(), "blue", "0x1010ff");
    let green = colour_clip(tmp.path(), "green", "0x10ff10");
    // a transition into the second clip, a plain cut into the third: 3 + 3 - 1 + 3
    let out = render(
        tmp.path(),
        "mixed",
        &[
            clip(&red, None),
            clip(&blue, fade(1000)),
            clip(&green, None),
        ],
        true,
    )
    .await;
    let len = secs(&out);
    assert!((len - 8.0).abs() < 0.4, "{len} s");
    let end = colour_at(&out, 7.5);
    assert!(
        end[1] > 180 && end[0] < 70 && end[2] < 70,
        "green last: {end:?}"
    );
    // two transitions in a row
    let out = render(
        tmp.path(),
        "twice",
        &[
            clip(&red, None),
            clip(&blue, fade(500)),
            clip(&green, fade(800)),
        ],
        true,
    )
    .await;
    let len = secs(&out);
    assert!((len - 7.7).abs() < 0.4, "{len} s");
}

#[tokio::test]
async fn every_kind_of_transition_renders_with_and_without_sound() {
    need_ffmpeg!();
    let tmp = tempfile::tempdir().unwrap();
    let red = colour_clip(tmp.path(), "red", "0xff1010");
    let blue = colour_clip(tmp.path(), "blue", "0x1010ff");
    let kinds = [
        TransitionKind::Fade,
        TransitionKind::FadeBlack,
        TransitionKind::FadeWhite,
        TransitionKind::WipeLeft,
        TransitionKind::WipeRight,
        TransitionKind::WipeUp,
        TransitionKind::WipeDown,
        TransitionKind::SlideLeft,
        TransitionKind::SlideRight,
        TransitionKind::SlideUp,
        TransitionKind::SlideDown,
        TransitionKind::CircleOpen,
        TransitionKind::CircleClose,
        TransitionKind::Radial,
        TransitionKind::Pixelize,
        TransitionKind::Dissolve,
    ];
    for (i, kind) in kinds.into_iter().enumerate() {
        let t = Some(Transition { kind, ms: 600 });
        let clips = [clip(&red, None), clip(&blue, t)];
        let out = render(tmp.path(), &format!("k{i}"), &clips, i % 2 == 0).await;
        let len = secs(&out);
        assert!((len - 5.4).abs() < 0.4, "{kind:?}: {len} s");
        assert_eq!(has_sound(&out), i % 2 == 0, "{kind:?}");
    }
}

#[tokio::test]
async fn a_clips_own_song_starts_where_its_clip_does_after_the_overlap() {
    need_ffmpeg!();
    let tmp = tempfile::tempdir().unwrap();
    let red = colour_clip(tmp.path(), "red", "0xff1010");
    let blue = colour_clip(tmp.path(), "blue", "0x1010ff");
    let song = song_file(tmp.path());
    let mut second = clip(&blue, fade(1000));
    second.music = Some(Soundtrack {
        id: "s".into(),
        start_ms: 0,
        at_ms: 0,
        volume: 1.0,
        fade_in_ms: 0,
        fade_out_ms: 0,
        duck: 0.0,
        curve: vec![],
        pan: 0.0,
        highpass_hz: 0.0,
        lowpass_hz: 0.0,
        file: Some(song),
    });
    // no sound of its own kept: only the song is heard, and it should come in with the second clip,
    // which starts at 2 s (3 s less the 1 s overlap)
    let out = render(tmp.path(), "song", &[clip(&red, None), second], false).await;
    assert!(
        level(&out, 0.2, 1.5) < -60.0,
        "silent before the second clip: {}",
        level(&out, 0.2, 1.5)
    );
    assert!(
        level(&out, 2.4, 1.2) > -30.0,
        "the song plays in the second clip: {}",
        level(&out, 2.4, 1.2)
    );
}

#[tokio::test]
async fn a_bad_transition_is_refused() {
    let spec = |t: Option<Transition>, len: i64| SequenceClipSpec {
        rating_key: "k".into(),
        start_ms: 0,
        end_ms: len,
        caption: None,
        subtitle_offset_ms: 0,
        layers: vec![],
        music: None,
        grade: Default::default(),
        transition: t,
    };
    assert!(export::validate_sequence(&[spec(None, 3000), spec(fade(1000), 3000)]).is_ok());
    let err = export::validate_sequence(&[spec(fade(1000), 3000), spec(None, 3000)]).unwrap_err();
    assert!(err.contains("first clip"), "{err}");
    let err = export::validate_sequence(&[spec(None, 3000), spec(fade(50), 3000)]).unwrap_err();
    assert!(err.contains("#2"), "{err}");
    let err = export::validate_sequence(&[spec(None, 3000), spec(fade(2500), 2000)]).unwrap_err();
    assert!(err.contains("too short"), "{err}");
}
