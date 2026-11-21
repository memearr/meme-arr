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
        duck: 0.0,
        curve: vec![],
        pan: 0.0,
        highpass_hz: 0.0,
        lowpass_hz: 0.0,
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
        clip_curve: vec![],
        mixdown: Default::default(),
        speed: 1.0,
        crop: None,
        grade: export::Grade::default(),
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
    if let Some(db) = text
        .lines()
        .find_map(|l| l.split("mean_volume:").nth(1))
        .and_then(|v| v.trim().trim_end_matches(" dB").parse().ok())
    {
        return db;
    }
    // Digital silence is encoded as next to no packets, so a window of it decodes no samples.
    if text.contains("n_samples: 0") {
        return -120.0;
    }
    panic!("no mean_volume in:\n{text}")
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

fn seq_clip(file: PathBuf, secs: i64) -> export::ResolvedSequenceClip {
    export::ResolvedSequenceClip {
        file,
        start_ms: 0,
        end_ms: secs * 1000,
        caption: None,
        subtitle_offset_ms: 0,
        cues: vec![],
        source: String::new(),
        font_path: None,
        layers: vec![],
        music: None,
        grade: Default::default(),
    }
}

async fn render_sequence(
    dir: &Path,
    name: &str,
    clips: &[export::ResolvedSequenceClip],
    req: export::SequenceExportRequest,
) -> anyhow::Result<PathBuf> {
    let out = dir.join(format!("{name}.{}", req.format.ext()));
    export::run_sequence_export(
        TOOLS,
        export::JobSpec {
            id: &format!("job-{name}"),
            tmp_dir: dir,
            out_path: &out,
            source_title: None,
        },
        &req,
        clips,
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/MemeTestSans.ttf"
        ),
    )
    .await?;
    Ok(out)
}

fn seq_request(
    format: Format,
    audio: bool,
    m: Option<Soundtrack>,
    clip_volume: f32,
) -> export::SequenceExportRequest {
    export::SequenceExportRequest {
        clips: vec![],
        format,
        width: 240,
        fps: 10,
        quality: 80,
        audio,
        filename: None,
        credit_source: false,
        music: m,
        clip_volume,
    }
}

#[tokio::test]
async fn a_song_plays_under_the_whole_sequence_and_stops_with_it() {
    need_ffmpeg!();
    let tmp = tempfile::tempdir().unwrap();
    let song = song_file(tmp.path());
    let (a, b) = (clip(tmp.path(), true), clip(tmp.path(), false));
    // 2 s of the toned clip, then 2 s of the silent one
    let clips = [seq_clip(a, 2), seq_clip(b, 2)];

    // music with the clips' sound kept: the second half, where the clips are silent, still has the song
    let out = render_sequence(
        tmp.path(),
        "both",
        &clips,
        seq_request(Format::Mp4, true, Some(music(&song)), 1.0),
    )
    .await
    .unwrap();
    let (has, secs) = audio_stream(&out);
    assert!(has && secs > 3.5 && secs < 4.4, "{has} {secs}");
    assert!(
        level(&out, 2.5, 1.0) > -30.0,
        "the song carries on over the silent clip"
    );
    assert!(level(&out, 0.3, 1.0) > -30.0);

    // music alone, the clips' sound not kept
    let alone = render_sequence(
        tmp.path(),
        "alone",
        &clips,
        seq_request(Format::Webm, false, Some(music(&song)), 1.0),
    )
    .await
    .unwrap();
    assert!(audio_stream(&alone).0);
    assert!(level(&alone, 0.5, 2.0) > -30.0);

    // the clips' own volume, no music: the first half is quieter by half
    let full = render_sequence(
        tmp.path(),
        "full",
        &clips,
        seq_request(Format::Mp4, true, None, 1.0),
    )
    .await
    .unwrap();
    let half = render_sequence(
        tmp.path(),
        "half",
        &clips,
        seq_request(Format::Mp4, true, None, 0.5),
    )
    .await
    .unwrap();
    let diff = level(&full, 0.3, 1.0) - level(&half, 0.3, 1.0);
    assert!(
        (diff - 6.0).abs() < 1.5,
        "half volume is 6 dB down ({diff})"
    );
}

#[tokio::test]
async fn a_sequence_refuses_music_in_a_gif_and_music_that_comes_in_after_the_end() {
    need_ffmpeg!();
    let tmp = tempfile::tempdir().unwrap();
    let song = song_file(tmp.path());
    let clips = [seq_clip(clip(tmp.path(), false), 2)];
    let err = render_sequence(
        tmp.path(),
        "gif",
        &clips,
        seq_request(Format::Gif, false, Some(music(&song)), 1.0),
    )
    .await
    .unwrap_err();
    assert!(format!("{err:#}").contains("MP4 or WebM"), "{err:#}");
    let mut late = music(&song);
    late.at_ms = 2000;
    let err = render_sequence(
        tmp.path(),
        "late",
        &clips,
        seq_request(Format::Mp4, false, Some(late), 1.0),
    )
    .await
    .unwrap_err();
    assert!(format!("{err:#}").contains("within the clip"), "{err:#}");
    // and nothing was left behind
    let left: Vec<_> = std::fs::read_dir(tmp.path())
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.contains("concat") || n.contains("list"))
        .collect();
    assert!(left.is_empty(), "{left:?}");
}

/// A 4 s clip with a 440 Hz tone for the first 2 s and silence after (like a line of dialogue).
fn speaking_clip(dir: &Path) -> PathBuf {
    let out = dir.join("speaking.mp4");
    let ok = Command::new("ffmpeg")
        .args(["-y", "-v", "error", "-f", "lavfi", "-i"])
        .arg("color=c=0x303030:duration=4:size=320x240:rate=10")
        .args([
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:duration=2,apad=whole_dur=4",
        ])
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

#[tokio::test]
async fn the_song_dips_while_the_clip_speaks_and_comes_back_after() {
    need_ffmpeg!();
    let tmp = tempfile::tempdir().unwrap();
    let (src, song) = (speaking_clip(tmp.path()), song_file(tmp.path()));
    let loud = |duck: f32| {
        let mut m = music(&song);
        m.volume = 2.0;
        m.duck = duck;
        m
    };
    let plain = render(
        tmp.path(),
        &src,
        "plain",
        request(Format::Mp4, true, Some(loud(0.0)), 1.0),
    )
    .await;
    let ducked = render(
        tmp.path(),
        &src,
        "ducked",
        request(Format::Mp4, true, Some(loud(1.0)), 1.0),
    )
    .await;
    let (p_talk, d_talk) = (level(&plain, 0.5, 1.2), level(&ducked, 0.5, 1.2));
    assert!(
        d_talk < p_talk - 4.0,
        "while the clip speaks the mix is quieter ({d_talk} dB vs {p_talk} dB)"
    );
    let (p_after, d_after) = (level(&plain, 3.0, 0.8), level(&ducked, 3.0, 0.8));
    assert!(
        (p_after - d_after).abs() < 1.0,
        "after it the song is back at full ({d_after} dB vs {p_after} dB)"
    );
    assert!(d_after > -30.0, "and audible");
}

#[tokio::test]
async fn ducking_needs_the_clips_sound() {
    need_ffmpeg!();
    let tmp = tempfile::tempdir().unwrap();
    let (src, song) = (clip(tmp.path(), true), song_file(tmp.path()));
    let mut m = music(&song);
    m.duck = 0.5;
    let out = tmp.path().join("no.mp4");
    let req = request(Format::Mp4, false, Some(m), 1.0);
    let err = export::run_export_with(
        TOOLS,
        export::JobSpec {
            id: "job-no",
            tmp_dir: tmp.path(),
            out_path: &out,
            source_title: None,
        },
        &src,
        &req,
        &[],
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/MemeTestSans.ttf"
        ),
        &|_: &str| -> Option<LayerSource> { None },
    )
    .await
    .unwrap_err();
    assert!(format!("{err:#}").contains("ducking"), "{err:#}");
    assert!(!out.exists());
}

fn with_song(mut c: export::ResolvedSequenceClip, m: Soundtrack) -> export::ResolvedSequenceClip {
    c.music = Some(m);
    c
}

#[tokio::test]
async fn a_song_on_one_clip_plays_under_that_clip_only_and_from_its_own_start() {
    need_ffmpeg!();
    let tmp = tempfile::tempdir().unwrap();
    let song = song_file(tmp.path());
    let src = clip(tmp.path(), false);
    // three 2 s clips; the middle one has a song that comes in 1 s into it
    let mut m = music(&song);
    m.at_ms = 1000;
    let clips = [
        seq_clip(src.clone(), 2),
        with_song(seq_clip(src.clone(), 2), m),
        seq_clip(src, 2),
    ];
    let out = render_sequence(
        tmp.path(),
        "mid",
        &clips,
        seq_request(Format::Mp4, false, None, 1.0),
    )
    .await
    .unwrap();
    assert!(
        level(&out, 0.2, 1.5) < -50.0,
        "nothing under the first clip"
    );
    assert!(
        level(&out, 2.2, 0.6) < -50.0,
        "nor before the song comes in"
    );
    assert!(
        level(&out, 3.2, 0.7) > -30.0,
        "the song is under the second clip's second half"
    );
    assert!(
        level(&out, 4.3, 1.5) < -50.0,
        "and it stops where its clip does, not carrying into the third"
    );
}

#[tokio::test]
async fn clip_songs_and_the_sequences_song_play_together() {
    need_ffmpeg!();
    let tmp = tempfile::tempdir().unwrap();
    let song = song_file(tmp.path());
    let src = clip(tmp.path(), false);
    let quiet = {
        let mut m = music(&song);
        m.volume = 0.2;
        m
    };
    let loud = music(&song);
    // the sequence's song is quiet all through; the second clip adds a loud one of its own
    let clips = [seq_clip(src.clone(), 2), with_song(seq_clip(src, 2), loud)];
    let out = render_sequence(
        tmp.path(),
        "both-songs",
        &clips,
        seq_request(Format::Mp4, false, Some(quiet), 1.0),
    )
    .await
    .unwrap();
    let (first, second) = (level(&out, 0.3, 1.4), level(&out, 2.3, 1.4));
    assert!(
        first > -40.0,
        "the sequence's own song is there under the first clip ({first} dB)"
    );
    assert!(
        second > first + 6.0,
        "the second clip's own song is added to it ({second} dB vs {first} dB)"
    );
}

#[tokio::test]
async fn a_clip_song_can_duck_under_its_own_clips_sound() {
    need_ffmpeg!();
    let tmp = tempfile::tempdir().unwrap();
    let song = song_file(tmp.path());
    let spoken = speaking_clip(tmp.path());
    let build = |duck: f32| {
        let mut m = music(&song);
        m.volume = 2.0;
        m.duck = duck;
        [with_song(seq_clip(spoken.clone(), 4), m)]
    };
    let plain = render_sequence(
        tmp.path(),
        "p",
        &build(0.0),
        seq_request(Format::Mp4, true, None, 1.0),
    )
    .await
    .unwrap();
    let ducked = render_sequence(
        tmp.path(),
        "d",
        &build(1.0),
        seq_request(Format::Mp4, true, None, 1.0),
    )
    .await
    .unwrap();
    assert!(
        level(&ducked, 0.5, 1.2) < level(&plain, 0.5, 1.2) - 4.0,
        "the song dips while the clip speaks"
    );
    assert!(
        (level(&ducked, 3.0, 0.8) - level(&plain, 3.0, 0.8)).abs() < 1.0,
        "and is back after"
    );
}

#[tokio::test]
async fn a_clip_song_is_refused_naming_the_clip_before_anything_is_rendered() {
    need_ffmpeg!();
    let tmp = tempfile::tempdir().unwrap();
    let song = song_file(tmp.path());
    let src = clip(tmp.path(), false);
    let mut late = music(&song);
    late.at_ms = 5000; // the clip is 2 s
    let clips = [
        seq_clip(src.clone(), 2),
        with_song(seq_clip(src.clone(), 2), late),
    ];
    let err = render_sequence(
        tmp.path(),
        "late",
        &clips,
        seq_request(Format::Mp4, false, None, 1.0),
    )
    .await
    .unwrap_err();
    let msg = format!("{err:#}");
    assert!(
        msg.contains("clip #2") && msg.contains("within the clip"),
        "{msg}"
    );
    // a GIF has no sound to put a song in
    let clips = [with_song(seq_clip(src, 2), music(&song))];
    let err = render_sequence(
        tmp.path(),
        "gif",
        &clips,
        seq_request(Format::Gif, false, None, 1.0),
    )
    .await
    .unwrap_err();
    assert!(format!("{err:#}").contains("clip #1"), "{err:#}");
    let left: Vec<_> = std::fs::read_dir(tmp.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.contains(".part") || n.starts_with("job-late") || n.starts_with("job-gif"))
        .collect();
    assert!(left.is_empty(), "nothing rendered or left behind: {left:?}");
}

// ---- the volume over time ---------------------------------------------------------

fn points(p: &[(i64, f32)]) -> Vec<export::VolumePoint> {
    p.iter()
        .map(|&(t_ms, gain)| export::VolumePoint { t_ms, gain })
        .collect()
}

#[tokio::test]
async fn the_songs_volume_line_dips_and_comes_back() {
    need_ffmpeg!();
    let tmp = tempfile::tempdir().unwrap();
    let (src, song) = (clip(tmp.path(), false), song_file(tmp.path()));
    let steady = render(
        tmp.path(),
        &src,
        "steady",
        request(Format::Mp4, false, Some(music(&song)), 1.0),
    )
    .await;
    let full = level(&steady, 0.5, 1.0);

    // full until 1 s, down to 10% by 2 s, back to full from 3 s to 3.5 s
    let mut m = music(&song);
    m.curve = points(&[(1000, 1.0), (2000, 0.1), (3000, 0.1), (3500, 1.0)]);
    let out = render(
        tmp.path(),
        &src,
        "dip",
        request(Format::Mp4, false, Some(m), 1.0),
    )
    .await;
    assert!(
        (level(&out, 0.2, 0.6) - full).abs() < 2.0,
        "full at the start"
    );
    let low = level(&out, 2.2, 0.6);
    assert!(
        (low - (full - 20.0)).abs() < 3.0,
        "a tenth of the volume is 20 dB down: {low} against {full}"
    );
    assert!(
        (level(&out, 3.6, 0.3) - full).abs() < 2.0,
        "and it comes back"
    );
}

#[tokio::test]
async fn the_volume_line_runs_on_the_clock_of_the_clip_not_the_songs_start() {
    need_ffmpeg!();
    let tmp = tempfile::tempdir().unwrap();
    let (src, song) = (clip(tmp.path(), false), song_file(tmp.path()));
    let steady = render(
        tmp.path(),
        &src,
        "steady2",
        request(Format::Mp4, false, Some(music(&song)), 1.0),
    )
    .await;
    let full = level(&steady, 0.5, 1.0);

    // the song comes in at 1 s; its line says silent until 2 s of the clip, full by 2.5 s
    let mut m = music(&song);
    m.at_ms = 1000;
    m.curve = points(&[(1000, 0.0), (2000, 0.0), (2500, 1.0)]);
    let out = render(
        tmp.path(),
        &src,
        "shifted",
        request(Format::Mp4, false, Some(m), 1.0),
    )
    .await;
    assert!(
        level(&out, 1.1, 0.8) < full - 40.0,
        "silent while the line is at 0"
    );
    assert!(
        (level(&out, 3.0, 0.8) - full).abs() < 2.0,
        "full after it has risen"
    );
}

#[tokio::test]
async fn the_clips_own_sound_can_have_a_volume_line_too() {
    need_ffmpeg!();
    let tmp = tempfile::tempdir().unwrap();
    let src = clip(tmp.path(), true);
    let plain = render(
        tmp.path(),
        &src,
        "own-plain",
        request(Format::Mp4, true, None, 1.0),
    )
    .await;
    let mut req = request(Format::Mp4, true, None, 1.0);
    req.clip_curve = points(&[(1000, 1.0), (2000, 0.0)]);
    let out = render(tmp.path(), &src, "own-line", req).await;
    assert!(
        (level(&out, 0.2, 0.6) - level(&plain, 0.2, 0.6)).abs() < 2.0,
        "untouched at the start"
    );
    assert!(
        level(&out, 2.5, 1.0) < level(&plain, 2.5, 1.0) - 40.0,
        "silent once the line is down"
    );
}

#[tokio::test]
async fn a_bad_volume_line_is_refused() {
    need_ffmpeg!();
    let tmp = tempfile::tempdir().unwrap();
    let (src, song) = (clip(tmp.path(), true), song_file(tmp.path()));
    for (name, curve) in [
        ("backwards", points(&[(2000, 0.5), (1000, 0.5)])),
        ("loud", points(&[(1000, 1.5)])),
        ("late", points(&[(9000, 0.5)])),
    ] {
        let mut m = music(&song);
        m.curve = curve;
        let req = request(Format::Mp4, true, Some(m), 1.0);
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
            &req,
            &[],
            "/nonexistent/font.ttf",
            &|_: &str| -> Option<LayerSource> { None },
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("volume line"), "{name}: {err}");
    }
}

/// The mean volume (dB) of one channel (0 left, 1 right) of the file's sound.
fn channel_level(file: &Path, ch: usize) -> f64 {
    let out = Command::new("ffmpeg")
        .args(["-v", "info", "-i"])
        .arg(file)
        .args(["-vn", "-af", &format!("pan=mono|c0=c{ch},volumedetect")])
        .args(["-f", "null", "-"])
        .output()
        .expect("spawn ffmpeg");
    let text = String::from_utf8_lossy(&out.stderr).to_string();
    text.lines()
        .find_map(|l| l.split("mean_volume:").nth(1))
        .and_then(|v| v.trim().trim_end_matches(" dB").parse().ok())
        .unwrap_or(-120.0)
}

fn mixed(clip_pan: f32, master: f32) -> export::Mixdown {
    export::Mixdown {
        clip_pan,
        master,
        ..Default::default()
    }
}

#[tokio::test]
async fn the_clips_own_sound_can_be_panned() {
    need_ffmpeg!();
    let tmp = tempfile::tempdir().unwrap();
    let src = clip(tmp.path(), true);
    let plain = render(
        tmp.path(),
        &src,
        "pan-plain",
        request(Format::Mp4, true, None, 1.0),
    )
    .await;
    let (pl, pr) = (channel_level(&plain, 0), channel_level(&plain, 1));
    assert!(
        (pl - pr).abs() < 1.0,
        "a plain mono tone is in both: {pl} {pr}"
    );

    for (name, pan) in [("left", -1.0f32), ("right", 1.0)] {
        let mut req = request(Format::Mp4, true, None, 1.0);
        req.mixdown = mixed(pan, 1.0);
        let out = render(tmp.path(), &src, &format!("pan-{name}"), req).await;
        let (l, r) = (channel_level(&out, 0), channel_level(&out, 1));
        let (loud, quiet) = if pan < 0.0 { (l, r) } else { (r, l) };
        assert!(quiet < -50.0, "{name}: the other side is silent ({quiet})");
        assert!(
            loud > pl - 1.0,
            "{name}: this side is as loud as before ({loud} vs {pl})"
        );
    }

    let mut req = request(Format::Mp4, true, None, 1.0);
    req.mixdown = mixed(-0.5, 1.0);
    let out = render(tmp.path(), &src, "pan-half", req).await;
    let (l, r) = (channel_level(&out, 0), channel_level(&out, 1));
    assert!(l > r + 5.0, "half left is left-heavy ({l} vs {r})");
    assert!(r > -50.0, "but not gone on the right ({r})");
}

#[tokio::test]
async fn a_song_can_be_panned_on_its_own() {
    need_ffmpeg!();
    let tmp = tempfile::tempdir().unwrap();
    let (src, song) = (clip(tmp.path(), true), song_file(tmp.path()));
    let mut m = music(&song);
    m.pan = 1.0;
    let out = render(
        tmp.path(),
        &src,
        "song-pan",
        request(Format::Mp4, true, Some(m), 0.0),
    )
    .await;
    // the clip's own sound is off (volume 0), so what is left is the song, all on the right
    let (l, r) = (channel_level(&out, 0), channel_level(&out, 1));
    assert!(l < -50.0 && r > -30.0, "the song is on the right ({l} {r})");
}

#[tokio::test]
async fn the_master_volume_scales_the_whole_mix() {
    need_ffmpeg!();
    let tmp = tempfile::tempdir().unwrap();
    let (src, song) = (clip(tmp.path(), true), song_file(tmp.path()));
    let make = |master: f32| {
        let mut req = request(Format::Mp4, true, Some(music(&song)), 0.5);
        req.mixdown = mixed(0.0, master);
        req
    };
    let full = render(tmp.path(), &src, "master-1", make(1.0)).await;
    let half = render(tmp.path(), &src, "master-half", make(0.5)).await;
    let off = render(tmp.path(), &src, "master-0", make(0.0)).await;
    let (a, b) = (level(&full, 0.5, 3.0), level(&half, 0.5, 3.0));
    assert!(
        (a - b - 6.0).abs() < 1.5,
        "half the master is about 6 dB down ({a} vs {b})"
    );
    assert!(level(&off, 0.5, 3.0) < -60.0, "master 0 is silence");
}

#[tokio::test]
async fn the_master_volume_alone_changes_the_clips_sound() {
    need_ffmpeg!();
    let tmp = tempfile::tempdir().unwrap();
    let src = clip(tmp.path(), true);
    let plain = render(
        tmp.path(),
        &src,
        "m-plain",
        request(Format::Mp4, true, None, 1.0),
    )
    .await;
    let mut req = request(Format::Mp4, true, None, 1.0);
    req.mixdown = mixed(0.0, 0.5);
    let quiet = render(tmp.path(), &src, "m-quiet", req).await;
    let (a, b) = (level(&plain, 0.5, 3.0), level(&quiet, 0.5, 3.0));
    assert!((a - b - 6.0).abs() < 1.5, "{a} vs {b}");
}

#[tokio::test]
async fn a_bad_pan_or_master_is_refused() {
    need_ffmpeg!();
    let tmp = tempfile::tempdir().unwrap();
    let (src, song) = (clip(tmp.path(), true), song_file(tmp.path()));
    for (name, audio, md, song_pan) in [
        ("pan-high", true, mixed(2.0, 1.0), 0.0f32),
        ("master-high", true, mixed(0.0, 3.0), 0.0),
        ("master-neg", true, mixed(0.0, -1.0), 0.0),
        ("pan-no-sound", false, mixed(0.5, 1.0), 0.0),
        ("song-pan", true, mixed(0.0, 1.0), 1.5),
    ] {
        let mut m = music(&song);
        m.pan = song_pan;
        let mut req = request(Format::Mp4, audio, Some(m), 1.0);
        req.mixdown = md;
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
            &req,
            &[],
            "/nonexistent/font.ttf",
            &|_: &str| -> Option<LayerSource> { None },
        )
        .await
        .unwrap_err();
        assert!(
            err.to_string().contains("pan") || err.to_string().contains("master"),
            "{name}: {err}"
        );
    }
}

#[tokio::test]
async fn cuts_take_the_low_end_off_the_clip_and_the_song() {
    need_ffmpeg!();
    let tmp = tempfile::tempdir().unwrap();
    let src = clip(tmp.path(), true);
    let plain = render(
        tmp.path(),
        &src,
        "cut-plain",
        request(Format::Mp4, true, None, 1.0),
    )
    .await;
    let mut req = request(Format::Mp4, true, None, 1.0);
    req.mixdown = export::Mixdown {
        clip_highpass_hz: 1500.0,
        ..Default::default()
    };
    let cut = render(tmp.path(), &src, "cut-clip", req).await;
    // the 440 Hz tone is well below the cut
    assert!(level(&plain, 0.5, 2.0) - level(&cut, 0.5, 2.0) > 10.0);
    // a low-pass above the tone leaves it be
    let mut req = request(Format::Mp4, true, None, 1.0);
    req.mixdown = export::Mixdown {
        clip_lowpass_hz: 5000.0,
        ..Default::default()
    };
    let low = render(tmp.path(), &src, "cut-low", req).await;
    assert!((level(&plain, 0.5, 2.0) - level(&low, 0.5, 2.0)).abs() < 1.0);

    // the song's own cut, on a 880 Hz tone
    let song = song_file(tmp.path());
    let mut m = music(&song);
    let open = render(
        tmp.path(),
        &src,
        "song-open",
        request(Format::Mp4, false, Some(m.clone()), 1.0),
    )
    .await;
    m.highpass_hz = 2000.0;
    let cut = render(
        tmp.path(),
        &src,
        "song-cut",
        request(Format::Mp4, false, Some(m), 1.0),
    )
    .await;
    assert!(level(&open, 0.5, 2.0) - level(&cut, 0.5, 2.0) > 8.0);
}

/// The integrated loudness (LUFS) of the file's sound.
async fn lufs(file: &Path) -> f32 {
    meme_arr::waveform::loudness("ffmpeg", file, 0, 600_000)
        .await
        .unwrap()
        .expect("some sound")
        .lufs
}

#[tokio::test]
async fn normalising_brings_a_quiet_and_a_loud_clip_to_the_same_loudness() {
    need_ffmpeg!();
    let tmp = tempfile::tempdir().unwrap();
    let src = clip(tmp.path(), true);
    let go = |name: &'static str, vol: f32, target: Option<f32>| {
        let mut req = request(Format::Mp4, true, None, vol);
        req.mixdown = export::Mixdown {
            normalize_lufs: target,
            ..Default::default()
        };
        (name, req)
    };
    let (n1, r1) = go("norm-quiet", 0.15, Some(-23.0));
    let (n2, r2) = go("norm-loud", 1.0, Some(-23.0));
    let (n3, r3) = go("norm-off", 0.15, None);
    let quiet = lufs(&render(tmp.path(), &src, n1, r1).await).await;
    let loud = lufs(&render(tmp.path(), &src, n2, r2).await).await;
    let untouched = lufs(&render(tmp.path(), &src, n3, r3).await).await;
    assert!((quiet - loud).abs() < 2.5, "quiet {quiet}, loud {loud}");
    assert!((loud - -23.0).abs() < 3.0, "{loud}");
    assert!(
        untouched < quiet - 6.0,
        "off stays quiet: {untouched} vs {quiet}"
    );
}
