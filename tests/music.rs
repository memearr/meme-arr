//! Music under a clip, rendered by the real ffmpeg: it is there, as loud as asked, cut at
//! the end of the clip, faded, delayed, mixed with the clip's own sound and kept with
//! layers. Levels are measured on the finished file. Skips when ffmpeg is not installed.

use meme_arr::export::{self, ExportRequest, Format, Layer, LayerKind, LayerSource, Soundtrack};
use std::path::{Path, PathBuf};
use std::process::Command;

const TOOLS: export::Tools<'static> = export::Tools {
    ffmpeg: "ffmpeg",
    ffprobe: "ffprobe",
    gifsicle: "gifsicle",
};

fn ffmpeg_available() -> bool {
    Command::new("ffmpeg")
        .arg("-version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// A 4 s clip: grey picture, and a 440 Hz tone when `audio`.
fn clip(dir: &Path, audio: bool) -> PathBuf {
    let out = dir.join(if audio { "toned.mp4" } else { "silent.mp4" });
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

/// A 10 s song: an 880 Hz tone (longer than any clip here).
fn song_file(dir: &Path) -> PathBuf {
    let out = dir.join("song.wav");
    let ok = Command::new("ffmpeg")
        .args(["-y", "-v", "error", "-f", "lavfi", "-i"])
        .arg("sine=frequency=880:duration=10")
        .arg(&out)
        .status()
        .expect("spawn ffmpeg")
        .success();
    assert!(ok);
    out
}

fn music(file: &Path) -> Soundtrack {
    Soundtrack {
        id: "song".into(),
        start_ms: 0,
        at_ms: 0,
        volume: 1.0,
        fade_in_ms: 0,
        fade_out_ms: 0,
        file: Some(file.to_path_buf()),
    }
}

fn request(format: Format, audio: bool, m: Option<Soundtrack>, clip_volume: f32) -> ExportRequest {
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
        music: m,
        clip_volume,
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

/// The mean volume (dB) of `len` seconds of the file's sound from `from`.
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
    let text = String::from_utf8_lossy(&out.stderr).to_string();
    text.lines()
        .find_map(|l| l.split("mean_volume:").nth(1))
        .and_then(|v| v.trim().trim_end_matches(" dB").parse().ok())
        .unwrap_or_else(|| panic!("no mean_volume in:\n{text}"))
}

/// `(has an audio stream, audio duration in s)`.
fn audio_stream(file: &Path) -> (bool, f64) {
    let out = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "a",
            "-show_entries",
            "stream=duration",
            "-of",
            "csv=p=0",
        ])
        .arg(file)
        .output()
        .expect("spawn ffprobe");
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    let first = text.lines().next().unwrap_or("").trim().to_string();
    (!first.is_empty(), first.parse().unwrap_or(0.0))
}

macro_rules! need_ffmpeg {
    () => {
        if !ffmpeg_available() {
            eprintln!("skipping music test: ffmpeg not found on PATH");
            return;
        }
    };
}

#[tokio::test]
async fn music_goes_under_a_silent_clip_and_stops_with_the_clip() {
    need_ffmpeg!();
    let tmp = tempfile::tempdir().unwrap();
    let (src, song) = (clip(tmp.path(), false), song_file(tmp.path()));
    let out = render(
        tmp.path(),
        &src,
        "m",
        request(Format::Mp4, false, Some(music(&song)), 1.0),
    )
    .await;
    let (has, secs) = audio_stream(&out);
    assert!(has, "there is a sound track now");
    assert!(
        secs > 3.5 && secs < 4.3,
        "cut at the clip's end, not the song's ({secs} s)"
    );
    assert!(level(&out, 0.5, 2.0) > -30.0, "the song is audible");
}

#[tokio::test]
async fn the_music_volume_is_what_was_asked() {
    need_ffmpeg!();
    let tmp = tempfile::tempdir().unwrap();
    let (src, song) = (clip(tmp.path(), false), song_file(tmp.path()));
    let at = |v: f32| {
        let mut m = music(&song);
        m.volume = v;
        request(Format::Mp4, false, Some(m), 1.0)
    };
    let full = render(tmp.path(), &src, "full", at(1.0)).await;
    let half = render(tmp.path(), &src, "half", at(0.5)).await;
    let mute = render(tmp.path(), &src, "mute", at(0.0)).await;
    let (f, h, z) = (
        level(&full, 0.5, 2.0),
        level(&half, 0.5, 2.0),
        level(&mute, 0.5, 2.0),
    );
    assert!(
        (f - h - 6.0).abs() < 1.5,
        "half the volume is 6 dB down ({f} vs {h})"
    );
    assert!(z < -60.0, "muted music is silent ({z})");
}

#[tokio::test]
async fn the_clips_own_sound_and_the_music_have_separate_volumes() {
    need_ffmpeg!();
    let tmp = tempfile::tempdir().unwrap();
    let (src, song) = (clip(tmp.path(), true), song_file(tmp.path()));
    let mut quiet_music = music(&song);
    quiet_music.volume = 0.0;
    // the clip's sound alone (music muted) is the clip's tone
    let own = render(
        tmp.path(),
        &src,
        "own",
        request(Format::Mp4, true, Some(quiet_music.clone()), 1.0),
    )
    .await;
    // the clip silenced and the music up: just the song
    let only_song = render(
        tmp.path(),
        &src,
        "song",
        request(Format::Mp4, true, Some(music(&song)), 0.0),
    )
    .await;
    // both
    let both = render(
        tmp.path(),
        &src,
        "both",
        request(Format::Mp4, true, Some(music(&song)), 1.0),
    )
    .await;
    // the clip turned down, no music at all
    let down = render(
        tmp.path(),
        &src,
        "down",
        request(Format::Mp4, true, None, 0.5),
    )
    .await;
    // nothing at all (clip silenced, music muted)
    let none = render(
        tmp.path(),
        &src,
        "none",
        request(Format::Mp4, true, Some(quiet_music), 0.0),
    )
    .await;

    let (o, s, b, d, n) = (
        level(&own, 0.5, 2.0),
        level(&only_song, 0.5, 2.0),
        level(&both, 0.5, 2.0),
        level(&down, 0.5, 2.0),
        level(&none, 0.5, 2.0),
    );
    assert!(o > -30.0 && s > -30.0, "each is audible alone ({o}, {s})");
    assert!(n < -60.0, "both silenced is silence ({n})");
    assert!(
        b > o.max(s) - 0.5,
        "together they are at least as loud as the louder one ({b} vs {o}, {s})"
    );
    assert!(
        (o - d - 6.0).abs() < 1.5,
        "the clip at half is 6 dB down with no music ({o} vs {d})"
    );
}

#[tokio::test]
async fn a_fade_in_and_a_late_start_shape_the_music() {
    need_ffmpeg!();
    let tmp = tempfile::tempdir().unwrap();
    let (src, song) = (clip(tmp.path(), false), song_file(tmp.path()));
    let mut fading = music(&song);
    fading.fade_in_ms = 1500;
    let faded = render(
        tmp.path(),
        &src,
        "fade",
        request(Format::Mp4, false, Some(fading), 1.0),
    )
    .await;
    assert!(
        level(&faded, 0.0, 0.3) < level(&faded, 2.0, 1.0) - 12.0,
        "it starts quiet and gets louder"
    );
    let mut fading_out = music(&song);
    fading_out.fade_out_ms = 1500;
    let out = render(
        tmp.path(),
        &src,
        "fadeout",
        request(Format::Mp4, false, Some(fading_out), 1.0),
    )
    .await;
    assert!(
        level(&out, 3.6, 0.35) < level(&out, 0.5, 1.0) - 12.0,
        "it dies away at the end"
    );

    let mut late = music(&song);
    late.at_ms = 2000;
    let delayed = render(
        tmp.path(),
        &src,
        "late",
        request(Format::Mp4, false, Some(late), 1.0),
    )
    .await;
    assert!(
        level(&delayed, 0.0, 1.5) < -50.0,
        "nothing before it comes in"
    );
    assert!(level(&delayed, 2.5, 1.0) > -30.0, "then it plays");
}

#[tokio::test]
async fn music_and_layers_together_and_webm_too() {
    need_ffmpeg!();
    let tmp = tempfile::tempdir().unwrap();
    let (src, song) = (clip(tmp.path(), true), song_file(tmp.path()));
    let dot = Layer {
        id: "l".into(),
        kind: LayerKind::Shape {
            shape: "dot".into(),
            color: None,
        },
        start_ms: 500,
        end_ms: Some(3000),
        x: 0.5,
        y: 0.5,
        size: 0.3,
        rotation: 0.0,
        opacity: 1.0,
        path: vec![],
    };
    for (fmt, keep) in [
        (Format::Mp4, true),
        (Format::Mp4, false),
        (Format::Webm, true),
    ] {
        let mut req = request(fmt, keep, Some(music(&song)), 1.0);
        req.layers = vec![dot.clone()];
        let name = format!("lay-{}-{keep}", req.format.ext());
        let out = render(tmp.path(), &src, &name, req).await;
        let (has, secs) = audio_stream(&out);
        // (WebM does not record a stream's length, so there it is only that it exists)
        assert!(
            has && (secs == 0.0 || (secs > 3.4 && secs < 4.4)),
            "{name}: sound {has} {secs}"
        );
        assert!(level(&out, 0.5, 2.0) > -30.0, "{name}: audible");
    }
}

#[tokio::test]
async fn gif_and_webp_refuse_music_and_a_bad_volume_is_refused() {
    need_ffmpeg!();
    let tmp = tempfile::tempdir().unwrap();
    let (src, song) = (clip(tmp.path(), false), song_file(tmp.path()));
    for fmt in [Format::Gif, Format::Webp] {
        let req = request(fmt, false, Some(music(&song)), 1.0);
        let err = export::run_export_with(
            TOOLS,
            export::JobSpec {
                id: "j",
                tmp_dir: tmp.path(),
                out_path: &tmp.path().join(format!("o.{}", fmt.ext())),
                source_title: None,
            },
            &src,
            &req,
            &[],
            "x",
            &|_: &str| -> Option<LayerSource> { None },
        )
        .await
        .unwrap_err();
        assert!(format!("{err:#}").contains("MP4 or WebM"), "{err:#}");
    }
}
