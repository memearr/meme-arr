//! A song's waveform from the real ffmpeg: a tone that goes quiet shows as tall then short
//! bars, and the length is right. Skips when ffmpeg is not installed.

use std::path::Path;
use std::process::Command;

fn ffmpeg_available() -> bool {
    Command::new("ffmpeg")
        .arg("-version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// 4 s: a loud 440 Hz tone for 2 s, then a quiet one.
fn song(dir: &Path) -> std::path::PathBuf {
    let out = dir.join("two-levels.wav");
    let ok = Command::new("ffmpeg")
        .args(["-y", "-v", "error", "-f", "lavfi", "-i"])
        .arg("sine=frequency=440:duration=2,volume=1.0[a];sine=frequency=440:duration=2,volume=0.1[b];[a][b]concat=n=2:v=0:a=1")
        .arg(&out)
        .status()
        .expect("spawn ffmpeg")
        .success();
    assert!(ok);
    out
}

#[tokio::test]
async fn a_loud_then_quiet_song_makes_tall_then_short_bars() {
    if !ffmpeg_available() {
        eprintln!("skipping waveform test: ffmpeg not found on PATH");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let w = meme_arr::waveform::analyze("ffmpeg", &song(tmp.path()), 40)
        .await
        .unwrap();
    assert!((w.duration_ms - 4000).abs() < 100, "{}", w.duration_ms);
    assert_eq!(w.peaks.len(), 40);
    let (first, second) = (&w.peaks[..18], &w.peaks[22..]);
    assert!(first.iter().all(|p| *p > 0.8), "{first:?}");
    assert!(second.iter().all(|p| *p < 0.3 && *p > 0.0), "{second:?}");
}

#[tokio::test]
async fn a_file_that_is_not_audio_is_an_error() {
    if !ffmpeg_available() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let f = tmp.path().join("x.mp3");
    std::fs::write(&f, b"not a song").unwrap();
    assert!(meme_arr::waveform::analyze("ffmpeg", &f, 40).await.is_err());
}
