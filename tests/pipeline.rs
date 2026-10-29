//! End-to-end pipeline test with NO Plex and NO real media: we synthesize a
//! tiny video with an embedded text subtitle track using ffmpeg, then run the
//! same extract -> parse -> export path the app uses.
//!
//! If ffmpeg/ffprobe aren't installed (e.g. a dev box without them) the test
//! skips rather than fails, so `cargo test` stays green everywhere. CI installs
//! ffmpeg so the real pipeline is exercised there.

use meme_arr::export::{Caption, ExportRequest, Format};
use meme_arr::{export, ffmpeg, subtitles};
use std::path::Path;
use std::process::Command;

fn ffmpeg_available() -> bool {
    Command::new("ffmpeg")
        .arg("-version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// `(codec_name, duration_ms)` of a file's first audio stream, straight from
/// ffprobe -- stricter than `Probe::has_audio`, which would also pass if the
/// source's AAC had been passed through instead of re-encoded to Opus.
fn first_audio_stream(path: &Path) -> Option<(String, i64)> {
    let out = Command::new("ffprobe")
        .args(["-v", "error", "-select_streams", "a:0"])
        .args(["-show_entries", "stream=codec_name,duration"])
        .args(["-of", "default=noprint_wrappers=1"])
        .arg(path)
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let field = |key: &str| {
        text.lines()
            .find_map(|l| l.strip_prefix(key))
            .map(|v| v.trim().to_string())
    };
    let codec = field("codec_name=")?;
    let duration_ms = field("duration=")
        .and_then(|d| d.parse::<f64>().ok())
        .map(|secs| (secs * 1000.0) as i64)
        .unwrap_or(0);
    Some((codec, duration_ms))
}

/// Build a 3s 320x240 test video with a mov_text subtitle track, optionally
/// with a sine-wave audio track (to exercise the audio export path for real,
/// and -- when `with_audio` is false for one sequence clip but not another
/// -- the silence-substitution branch that keeps concat segments consistent).
fn make_sample(dir: &Path, name: &str, with_audio: bool) -> std::path::PathBuf {
    let srt = dir.join(format!("{name}.srt"));
    std::fs::write(
        &srt,
        "1\n00:00:00,500 --> 00:00:01,500\nI am the one who knocks\n\n\
         2\n00:00:02,000 --> 00:00:02,800\nSay my name\n",
    )
    .unwrap();

    let out = dir.join(format!("{name}.mp4"));
    let mut cmd = Command::new("ffmpeg");
    cmd.args([
        "-y",
        "-v",
        "error",
        "-f",
        "lavfi",
        "-i",
        "testsrc=duration=3:size=320x240:rate=10",
    ]);
    if with_audio {
        cmd.args(["-f", "lavfi", "-i", "sine=frequency=440:duration=3"]);
    }
    cmd.args(["-i", &srt.to_string_lossy()]);
    if with_audio {
        cmd.args(["-c:a", "aac"]);
    }
    cmd.args([
        "-c:v",
        "libx264",
        "-pix_fmt",
        "yuv420p",
        "-c:s",
        "mov_text",
        "-shortest",
        &out.to_string_lossy(),
    ]);
    let status = cmd.status().expect("spawn ffmpeg");
    assert!(status.success(), "sample generation failed");
    out
}

#[tokio::test]
async fn extract_parse_and_export() {
    if !ffmpeg_available() {
        eprintln!("skipping pipeline test: ffmpeg not found on PATH");
        return;
    }

    let tmp = tempfile::tempdir().unwrap();
    let sample = make_sample(tmp.path(), "sample", false);

    // 1. Probe: exactly one, text-based subtitle track.
    let probe = ffmpeg::probe("ffprobe", &sample).await.expect("probe");
    assert!(
        probe.duration_ms >= 2500,
        "duration = {}",
        probe.duration_ms
    );
    assert_eq!(probe.subtitle_tracks.len(), 1);
    let track = ffmpeg::pick_track(&probe.subtitle_tracks).expect("a text track");

    // 2. Extract + parse the cues.
    let srt = ffmpeg::extract_srt("ffmpeg", &sample, track.rel_index)
        .await
        .expect("extract srt");
    let cues = subtitles::parse(&srt);
    assert!(
        cues.iter().any(|c| c.text.contains("one who knocks")),
        "expected the indexed line, got: {cues:?}"
    );

    // 3. Export a GIF of the clip window (no captions -> no font dependency).
    let req = ExportRequest {
        rating_key: "test".into(),
        start_ms: 300,
        end_ms: 1800,
        format: Format::Gif,
        width: 240,
        fps: 10,
        quality: 80,
        caption: Some(Caption::None),
        subtitle_offset_ms: 0,
        audio: false,
        filename: Some("knocks".into()),
        credit_source: true,
    };
    let out = tmp.path().join("knocks.gif");
    export::run_export(
        "ffmpeg",
        "gifsicle",
        tmp.path(),
        "job-test",
        &sample,
        &req,
        &cues,
        "/nonexistent/font.ttf",
        135,
        None,
        &out,
    )
    .await
    .expect("export");

    let meta = std::fs::metadata(&out).expect("gif written");
    assert!(meta.len() > 0, "gif is empty");
}

#[tokio::test]
async fn export_with_audio_produces_an_opus_track() {
    if !ffmpeg_available() {
        eprintln!("skipping pipeline test: ffmpeg not found on PATH");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let sample = make_sample(tmp.path(), "audio-sample", true);

    let req = ExportRequest {
        rating_key: "test".into(),
        start_ms: 300,
        end_ms: 1800,
        format: Format::Mp4,
        width: 240,
        fps: 10,
        quality: 80,
        caption: Some(Caption::None),
        subtitle_offset_ms: 0,
        audio: true,
        filename: Some("with-audio".into()),
        credit_source: true,
    };
    let out = tmp.path().join("with-audio.mp4");
    export::run_export(
        "ffmpeg",
        "gifsicle",
        tmp.path(),
        "job-audio-test",
        &sample,
        &req,
        &[],
        "/nonexistent/font.ttf",
        135,
        None,
        &out,
    )
    .await
    .expect("export");

    let (codec, _) = first_audio_stream(&out).expect("expected an audio stream in the export");
    assert_eq!(codec, "opus", "source AAC should be re-encoded to Opus");
}

/// The riskiest part of sequence audio: clips with and without their own
/// audio track, concatenated together. Confirms the silence-substitution
/// path (src/export.rs's `sequence_clip_args_with_audio`) actually keeps the
/// concat demuxer happy end to end, not just that its arg-building looks
/// right in isolation.
#[tokio::test]
async fn sequence_export_with_audio_handles_a_mute_clip() {
    if !ffmpeg_available() {
        eprintln!("skipping pipeline test: ffmpeg not found on PATH");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let with_audio = make_sample(tmp.path(), "seq-a", true);
    let without_audio = make_sample(tmp.path(), "seq-b", false);

    let clips = vec![
        export::ResolvedSequenceClip {
            file: with_audio,
            start_ms: 0,
            end_ms: 1000,
            caption: None,
            subtitle_offset_ms: 0,
            cues: vec![],
            source: String::new(),
            font_path: None,
        },
        export::ResolvedSequenceClip {
            file: without_audio,
            start_ms: 0,
            end_ms: 1000,
            caption: None,
            subtitle_offset_ms: 0,
            cues: vec![],
            source: String::new(),
            font_path: None,
        },
    ];

    let out = tmp.path().join("sequence-with-audio.mp4");
    export::run_sequence_export(
        "ffmpeg",
        "gifsicle",
        "ffprobe",
        tmp.path(),
        "job-seq-audio-test",
        &clips,
        Format::Mp4,
        240,
        10,
        80,
        true,
        "/nonexistent/font.ttf",
        None,
        &out,
    )
    .await
    .expect("sequence export");

    let probe = ffmpeg::probe("ffprobe", &out).await.expect("probe output");
    assert!(
        probe.duration_ms >= 1800,
        "duration = {}",
        probe.duration_ms
    );
    let (codec, audio_ms) =
        first_audio_stream(&out).expect("expected an audio stream even though one clip was silent");
    assert_eq!(codec, "opus");
    // The silent clip's segment must carry (substituted) audio too -- if it
    // were dropped, the audio would end after the first clip, ~1s in.
    assert!(
        audio_ms >= 1800,
        "audio covers only {audio_ms}ms of a ~2s sequence"
    );
}

// ---- exports carry none of the source file's metadata ----------------------

const SECRETS: &[&str] = &[
    "SECRET",
    "REMUX",
    "Ripped",
    "2160p",
    "CHAPTERSECRET",
    "/media/",
];

/// A source that looks like a movie rip: a release-name title, a comment, track
/// and chapter titles, a creation time, an audio track and a soft-subtitle track.
fn make_tagged_sample(dir: &Path, name: &str) -> std::path::PathBuf {
    let srt = dir.join(format!("{name}.srt"));
    std::fs::write(&srt, "1\n00:00:00,500 --> 00:00:01,500\nhello\n").unwrap();
    let chapters = dir.join(format!("{name}-chapters.txt"));
    std::fs::write(
        &chapters,
        ";FFMETADATA1\n[CHAPTER]\nTIMEBASE=1/1000\nSTART=0\nEND=2000\ntitle=CHAPTERSECRET\n",
    )
    .unwrap();
    let out = dir.join(format!("{name}.mkv"));
    let status = Command::new("ffmpeg")
        .args(["-y", "-v", "error"])
        .args([
            "-f",
            "lavfi",
            "-i",
            "testsrc=duration=3:size=320x240:rate=10",
        ])
        .args(["-f", "lavfi", "-i", "sine=frequency=440:duration=3"])
        .arg("-i")
        .arg(&srt)
        .arg("-i")
        .arg(&chapters)
        .args(["-map", "0:v", "-map", "1:a", "-map", "2:s"])
        .args(["-map_metadata", "3", "-map_chapters", "3"])
        .args(["-metadata", "title=SECRET.Movie.2021.2160p.REMUX-GRP"])
        .args(["-metadata", "comment=Ripped by SomeGroup"])
        .args(["-metadata", "creation_time=2020-01-02T03:04:05Z"])
        .args(["-metadata:s:v:0", "title=Video SECRET"])
        .args(["-metadata:s:a:0", "title=Audio SECRET"])
        .args(["-metadata:s:s:0", "title=Subs SECRET"])
        .args([
            "-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac", "-c:s", "srt",
        ])
        .arg(&out)
        .status()
        .expect("spawn ffmpeg");
    assert!(status.success(), "tagged sample generation failed");
    out
}

/// Every tag ffprobe can see (container, streams, chapters) as `(key, value)`.
fn all_tags(path: &Path) -> Vec<(String, String)> {
    let out = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "format_tags:stream_tags:chapter_tags",
        ])
        .args(["-of", "default=noprint_wrappers=1"])
        .arg(path)
        .output()
        .expect("ffprobe");
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| l.strip_prefix("TAG:"))
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| (k.to_lowercase(), v.to_string()))
        .collect()
}

fn stream_kinds(path: &Path) -> Vec<String> {
    let out = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "stream=codec_type",
            "-of",
            "csv=p=0",
        ])
        .arg(path)
        .output()
        .expect("ffprobe");
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|l| l.trim().to_string())
        .collect()
}

fn chapter_count(path: &Path) -> usize {
    let out = Command::new("ffprobe")
        .args(["-v", "error", "-show_chapters", "-of", "csv=p=0"])
        .arg(path)
        .output()
        .expect("ffprobe");
    String::from_utf8_lossy(&out.stdout).lines().count()
}

/// Nothing of the source in the file: not as a tag, not anywhere in its bytes.
fn assert_nothing_of_the_source(path: &Path) {
    let bytes = std::fs::read(path).unwrap();
    for needle in SECRETS {
        assert!(
            !bytes.windows(needle.len()).any(|w| w == needle.as_bytes()),
            "{} contains {needle:?}",
            path.display()
        );
    }
    for (k, v) in all_tags(path) {
        assert!(
            !SECRETS.iter().any(|s| v.contains(s)),
            "{}: tag {k}={v}",
            path.display()
        );
        // the only tags left are structural ones, or the allowed title
        let ok = matches!(
            k.as_str(),
            "title"
                | "major_brand"
                | "minor_version"
                | "compatible_brands"
                | "handler_name"
                | "language"
                | "duration"
                | "encoder"
        );
        // ffmpeg 6.x's MP4 muxer writes an all-zero vendor_id on a stream: structure, not source data
        let ok = ok || (k == "vendor_id" && v == "[0][0][0][0]");
        assert!(ok, "{}: unexpected tag {k}={v}", path.display());
    }
    assert_eq!(
        chapter_count(path),
        0,
        "chapters survived in {}",
        path.display()
    );
    assert!(
        stream_kinds(path)
            .iter()
            .all(|k| k == "video" || k == "audio"),
        "{}: streams {:?}",
        path.display(),
        stream_kinds(path)
    );
}

#[tokio::test]
async fn exports_keep_nothing_from_the_source_but_the_credited_title() {
    if !ffmpeg_available() {
        eprintln!("skipping pipeline test: ffmpeg not found on PATH");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let source = make_tagged_sample(tmp.path(), "rip");
    // the source really does carry all of it
    let src_tags = all_tags(&source);
    assert!(
        src_tags.iter().any(|(_, v)| v.contains("SECRET")),
        "{src_tags:?}"
    );
    assert_eq!(chapter_count(&source), 1);
    assert!(stream_kinds(&source).contains(&"subtitle".to_string()));

    for (format, name, audio) in [
        (Format::Mp4, "a.mp4", true),
        (Format::Webm, "b.webm", true),
        (Format::Gif, "c.gif", false),
        (Format::Webp, "d.webp", false),
    ] {
        let req = ExportRequest {
            rating_key: "t".into(),
            start_ms: 200,
            end_ms: 2200,
            format,
            width: 240,
            fps: 10,
            quality: 70,
            caption: Some(Caption::None),
            subtitle_offset_ms: 0,
            audio,
            filename: None,
            credit_source: true,
        };
        let out = tmp.path().join(name);
        export::run_export(
            "ffmpeg",
            "gifsicle",
            tmp.path(),
            &format!("job-{name}"),
            &source,
            &req,
            &[],
            "/nonexistent/font.ttf",
            135,
            Some("The Lantern Cove - S02E03 - Tideline"),
            &out,
        )
        .await
        .unwrap_or_else(|e| panic!("{name}: {e:#}"));
        assert_nothing_of_the_source(&out);
        let titles: Vec<String> = all_tags(&out)
            .into_iter()
            .filter(|(k, _)| k == "title")
            .map(|(_, v)| v)
            .collect();
        if matches!(format, Format::Mp4 | Format::Webm) {
            assert_eq!(titles, ["The Lantern Cove - S02E03 - Tideline"], "{name}");
        } else {
            assert!(
                titles.is_empty(),
                "{name} has nowhere to keep a title: {titles:?}"
            );
        }
        if audio {
            assert!(
                first_audio_stream(&out).is_some(),
                "{name} should keep its audio"
            );
        }
    }

    // opting out: no title either
    let req = ExportRequest {
        rating_key: "t".into(),
        start_ms: 200,
        end_ms: 2200,
        format: Format::Mp4,
        width: 240,
        fps: 10,
        quality: 70,
        caption: Some(Caption::None),
        subtitle_offset_ms: 0,
        audio: false,
        filename: None,
        credit_source: false,
    };
    let out = tmp.path().join("plain.mp4");
    export::run_export(
        "ffmpeg",
        "gifsicle",
        tmp.path(),
        "job-plain",
        &source,
        &req,
        &[],
        "/nonexistent/font.ttf",
        135,
        Some("The Lantern Cove - S02E03 - Tideline"),
        &out,
    )
    .await
    .unwrap();
    assert_nothing_of_the_source(&out);
    assert!(
        all_tags(&out).iter().all(|(k, _)| k != "title"),
        "{:?}",
        all_tags(&out)
    );
}

#[tokio::test]
async fn sequence_exports_keep_nothing_from_their_sources_either() {
    if !ffmpeg_available() {
        eprintln!("skipping pipeline test: ffmpeg not found on PATH");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let a = make_tagged_sample(tmp.path(), "rip-a");
    let b = make_tagged_sample(tmp.path(), "rip-b");
    let clip = |file: std::path::PathBuf, source: &str| export::ResolvedSequenceClip {
        file,
        start_ms: 0,
        end_ms: 1000,
        caption: None,
        subtitle_offset_ms: 0,
        cues: vec![],
        source: source.into(),
        font_path: None,
    };
    let clips = vec![clip(a, "A (2019)"), clip(b, "B - S01E02 - X")];
    for (format, name, audio) in [
        (Format::Mp4, "s.mp4", true),
        (Format::Webm, "s.webm", false),
        (Format::Gif, "s.gif", false),
    ] {
        let out = tmp.path().join(name);
        export::run_sequence_export(
            "ffmpeg",
            "gifsicle",
            "ffprobe",
            tmp.path(),
            &format!("job-seq-{name}"),
            &clips,
            format,
            240,
            10,
            70,
            audio,
            "/nonexistent/font.ttf",
            Some("A (2019) / B - S01E02 - X"),
            &out,
        )
        .await
        .unwrap_or_else(|e| panic!("{name}: {e:#}"));
        assert_nothing_of_the_source(&out);
        if matches!(format, Format::Mp4 | Format::Webm) {
            let titles: Vec<String> = all_tags(&out)
                .into_iter()
                .filter(|(k, _)| k == "title")
                .map(|(_, v)| v)
                .collect();
            assert_eq!(titles, ["A (2019) / B - S01E02 - X"], "{name}");
        }
    }
}

// ---- WebM with audio, for real ---------------------------------------------

/// `codec_name` of the first stream of the given kind (`v:0` / `a:0`).
fn first_codec(path: &Path, select: &str) -> Option<String> {
    let out = Command::new("ffprobe")
        .args(["-v", "error", "-select_streams", select])
        .args(["-show_entries", "stream=codec_name", "-of", "csv=p=0"])
        .arg(path)
        .output()
        .ok()?;
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!s.is_empty()).then_some(s)
}

/// Decode every frame and sample; true if ffmpeg got through it all.
fn decodes_fully(path: &Path) -> bool {
    Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(path)
        .args(["-f", "null", "-"])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// What ffmpeg complains about (warnings and up) when it re-muxes the file, e.g.
/// the non-monotonic audio timestamps that sequence joins used to leave behind.
fn remux_warnings(path: &Path) -> String {
    let out = Command::new("ffmpeg")
        .args(["-v", "warning", "-i"])
        .arg(path)
        .args(["-c", "copy", "-f", "null", "-"])
        .output()
        .expect("ffmpeg");
    String::from_utf8_lossy(&out.stderr).trim().to_string()
}

/// Mean volume in dB of a file's audio: far below -60 means silence.
fn mean_volume_db(path: &Path) -> Option<f64> {
    let out = Command::new("ffmpeg")
        .args(["-v", "info", "-i"])
        .arg(path)
        .args(["-vn", "-af", "volumedetect", "-f", "null", "-"])
        .output()
        .ok()?;
    String::from_utf8_lossy(&out.stderr)
        .lines()
        .find_map(|l| l.split("mean_volume:").nth(1))
        .and_then(|v| v.trim().trim_end_matches(" dB").trim().parse().ok())
}

#[tokio::test]
async fn webm_export_with_audio_is_vp9_plus_opus_with_real_sound() {
    if !ffmpeg_available() {
        eprintln!("skipping pipeline test: ffmpeg not found on PATH");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let sample = make_sample(tmp.path(), "webm-src", true);
    let req = ExportRequest {
        rating_key: "t".into(),
        start_ms: 300,
        end_ms: 2300,
        format: Format::Webm,
        width: 240,
        fps: 10,
        quality: 70,
        caption: Some(Caption::None),
        subtitle_offset_ms: 0,
        audio: true,
        filename: None,
        credit_source: true,
    };
    let out = tmp.path().join("clip.webm");
    export::run_export(
        "ffmpeg",
        "gifsicle",
        tmp.path(),
        "job-webm-audio",
        &sample,
        &req,
        &[],
        "/nonexistent/font.ttf",
        135,
        None,
        &out,
    )
    .await
    .expect("webm export");
    assert_eq!(first_codec(&out, "v:0").as_deref(), Some("vp9"));
    assert_eq!(first_codec(&out, "a:0").as_deref(), Some("opus"));
    assert!(decodes_fully(&out), "the WebM doesn't decode cleanly");
    // the source is a sine tone: the audio that came through is audible, not silence
    assert!(
        mean_volume_db(&out).is_some_and(|v| v > -40.0),
        "{:?}",
        mean_volume_db(&out)
    );
    // and it is as long as the video (about 2 s), not cut short
    let (_, audio_ms) = first_audio_stream(&out).expect("audio stream");
    let probe = ffmpeg::probe("ffprobe", &out).await.expect("probe");
    assert!(probe.duration_ms >= 1800, "duration {}", probe.duration_ms);
    assert!(
        audio_ms == 0 || audio_ms >= 1800,
        "audio only {audio_ms} ms"
    ); // WebM may not report a per-stream duration
}

#[tokio::test]
async fn webm_sequence_with_audio_keeps_sound_across_a_silent_clip() {
    if !ffmpeg_available() {
        eprintln!("skipping pipeline test: ffmpeg not found on PATH");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let loud = make_sample(tmp.path(), "wseq-a", true);
    let mute = make_sample(tmp.path(), "wseq-b", false);
    let clip = |file: std::path::PathBuf| export::ResolvedSequenceClip {
        file,
        start_ms: 0,
        end_ms: 1500,
        caption: None,
        subtitle_offset_ms: 0,
        cues: vec![],
        source: String::new(),
        font_path: None,
    };
    let clips = vec![clip(loud.clone()), clip(mute), clip(loud)];
    let out = tmp.path().join("seq.webm");
    export::run_sequence_export(
        "ffmpeg",
        "gifsicle",
        "ffprobe",
        tmp.path(),
        "job-webm-seq-audio",
        &clips,
        Format::Webm,
        240,
        10,
        70,
        true,
        "/nonexistent/font.ttf",
        None,
        &out,
    )
    .await
    .expect("webm sequence export");
    assert_eq!(first_codec(&out, "v:0").as_deref(), Some("vp9"));
    assert_eq!(first_codec(&out, "a:0").as_deref(), Some("opus"));
    assert!(decodes_fully(&out), "the WebM sequence doesn't decode");
    assert_eq!(remux_warnings(&out), "", "timestamps at the clip joins");
    let probe = ffmpeg::probe("ffprobe", &out).await.expect("probe");
    assert!(
        probe.duration_ms >= 4200,
        "3 x 1.5 s expected, got {} ms",
        probe.duration_ms
    );
    assert!(
        mean_volume_db(&out).is_some_and(|v| v > -45.0),
        "{:?}",
        mean_volume_db(&out)
    );

    // the same sequence as MP4: joins clean, and the audio runs as long as the video
    let mp4 = tmp.path().join("seq.mp4");
    export::run_sequence_export(
        "ffmpeg",
        "gifsicle",
        "ffprobe",
        tmp.path(),
        "job-mp4-seq-audio",
        &clips,
        Format::Mp4,
        240,
        10,
        70,
        true,
        "/nonexistent/font.ttf",
        None,
        &mp4,
    )
    .await
    .expect("mp4 sequence export");
    assert_eq!(remux_warnings(&mp4), "");
    let (_, audio_ms) = first_audio_stream(&mp4).expect("audio stream");
    let probe = ffmpeg::probe("ffprobe", &mp4).await.expect("probe");
    assert!(
        (audio_ms - probe.duration_ms).abs() < 60,
        "audio {audio_ms} ms vs video {} ms",
        probe.duration_ms
    );
}

#[tokio::test]
async fn burned_subtitles_over_a_window_with_no_lines_export_without_captions() {
    if !ffmpeg_available() {
        eprintln!("skipping pipeline test: ffmpeg not found on PATH");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let sample = make_sample(tmp.path(), "nolines-src", false);
    // the only line is at 2.5 s; the clip is 0.2-1.2 s
    let cues = vec![subtitles::Cue {
        start_ms: 2500,
        end_ms: 2900,
        text: "outside the window".into(),
    }];
    for (format, name) in [(Format::Mp4, "a.mp4"), (Format::Gif, "b.gif")] {
        let req = ExportRequest {
            rating_key: "t".into(),
            start_ms: 200,
            end_ms: 1200,
            format,
            width: 240,
            fps: 10,
            quality: 70,
            caption: Some(Caption::source(None)),
            subtitle_offset_ms: 0,
            audio: false,
            filename: None,
            credit_source: true,
        };
        let out = tmp.path().join(name);
        export::run_export(
            "ffmpeg",
            "gifsicle",
            tmp.path(),
            &format!("job-{name}"),
            &sample,
            &req,
            &cues,
            "/nonexistent/font.ttf",
            135,
            None,
            &out,
        )
        .await
        .unwrap_or_else(|e| {
            panic!("{name}: an empty caption window must not fail the export: {e:#}")
        });
        assert!(decodes_fully(&out), "{name} should be a valid file");
    }
    // and no caption temp files were left behind
    let leftovers: Vec<_> = std::fs::read_dir(tmp.path())
        .unwrap()
        .flatten()
        .filter(|e| {
            e.path().extension().is_some_and(|x| x == "srt")
                && e.file_name().to_string_lossy().starts_with("job-")
        })
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}

#[tokio::test]
async fn a_chosen_font_and_size_are_used_for_both_kinds_of_caption() {
    if !ffmpeg_available() {
        eprintln!("skipping pipeline test: ffmpeg not found on PATH");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let sample = make_sample(tmp.path(), "font-src", false);
    let font = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/MemeTestSans.ttf"
    );
    let cues = vec![subtitles::Cue {
        start_ms: 0,
        end_ms: 1500,
        text: "Hello there".into(),
    }];
    let captions = [
        (
            "subs",
            Caption::Source {
                cues: None,
                font: Some("MemeTestSans.ttf".into()),
                font_scale: 160,
            },
        ),
        (
            "meme",
            Caption::Meme {
                top: Some("TOP TEXT".into()),
                bottom: Some("Bottom".into()),
                font_size: 36,
                font: Some("MemeTestSans.ttf".into()),
                font_scale: 60,
            },
        ),
    ];
    let mut sizes = Vec::new();
    for (name, caption) in captions {
        let req = ExportRequest {
            rating_key: "t".into(),
            start_ms: 0,
            end_ms: 1500,
            format: Format::Mp4,
            width: 320,
            fps: 10,
            quality: 70,
            caption: Some(caption),
            subtitle_offset_ms: 0,
            audio: false,
            filename: None,
            credit_source: false,
        };
        let out = tmp.path().join(format!("{name}.mp4"));
        export::run_export(
            "ffmpeg",
            "gifsicle",
            tmp.path(),
            &format!("job-{name}"),
            &sample,
            &req,
            &cues,
            font,
            180,
            None,
            &out,
        )
        .await
        .unwrap_or_else(|e| panic!("{name}: {e:#}"));
        assert!(decodes_fully(&out), "{name}");
        sizes.push(std::fs::metadata(&out).unwrap().len());
    }
    // the same clip with no caption at all, to show the text really was drawn
    let plain = ExportRequest {
        rating_key: "t".into(),
        start_ms: 0,
        end_ms: 1500,
        format: Format::Mp4,
        width: 320,
        fps: 10,
        quality: 70,
        caption: Some(Caption::None),
        subtitle_offset_ms: 0,
        audio: false,
        filename: None,
        credit_source: false,
    };
    let out = tmp.path().join("plain.mp4");
    export::run_export(
        "ffmpeg",
        "gifsicle",
        tmp.path(),
        "job-plain-font",
        &sample,
        &plain,
        &[],
        font,
        180,
        None,
        &out,
    )
    .await
    .unwrap();
    let plain_size = std::fs::metadata(&out).unwrap().len();
    assert!(
        sizes.iter().all(|s| *s != plain_size),
        "captions should change the picture: {sizes:?} vs {plain_size}"
    );
}
