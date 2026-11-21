//! A song's loudness over time, as a handful of bars the editor draws so you can pick where
//! in the song to start by eye. The song is decoded by ffmpeg to a low-rate mono stream and
//! each bar is the loudest sample in its slice, scaled so the loudest bar is 1.

use anyhow::{bail, Context, Result};
use std::path::Path;

/// The sample rate the song is read at: plenty for loudness, little to hold.
pub const RATE: i64 = 4000;
/// Fewest and most bars a request may ask for.
pub const MIN_BINS: usize = 20;
pub const MAX_BINS: usize = 2000;
/// Longest stretch of a song looked at (2 hours: more than any song).
const MAX_SECS: u32 = 2 * 3600;

/// What the editor draws.
#[derive(Debug, Clone, PartialEq)]
pub struct Wave {
    /// How long the song is, as ffmpeg decoded it.
    pub duration_ms: i64,
    /// `bins` values, 0 silent .. 1 loudest, three decimals.
    pub peaks: Vec<f32>,
}

/// `bins` peaks of `samples`: the loudest sample in each slice, scaled to the loudest of all.
/// Fewer samples than bins gives one bar per sample; no samples gives no bars.
pub fn peaks(samples: &[i16], bins: usize) -> Vec<f32> {
    let bins = bins.min(samples.len());
    if bins == 0 {
        return Vec::new();
    }
    let loud: Vec<u32> = (0..bins)
        .map(|i| {
            let (a, b) = (i * samples.len() / bins, (i + 1) * samples.len() / bins);
            samples[a..b.max(a + 1)]
                .iter()
                .map(|s| s.unsigned_abs() as u32)
                .max()
                .unwrap_or(0)
        })
        .collect();
    let top = loud.iter().copied().max().unwrap_or(0);
    if top == 0 {
        return vec![0.0; bins];
    }
    loud.iter()
        .map(|&v| ((v as f32 / top as f32) * 1000.0).round() / 1000.0)
        .collect()
}

/// Little-endian 16-bit samples from ffmpeg's raw output.
fn samples_of(bytes: &[u8]) -> Vec<i16> {
    bytes
        .chunks_exact(2)
        .map(|c| i16::from_le_bytes([c[0], c[1]]))
        .collect()
}

/// Decode `file` and summarise it as `bins` bars.
pub async fn analyze(ffmpeg: &str, file: &Path, bins: usize) -> Result<Wave> {
    analyze_span(ffmpeg, file, 0, MAX_SECS as i64 * 1000, bins).await
}

/// `analyze`, for the `len_ms` from `start_ms` only (a clip's sound out of a whole film).
pub async fn analyze_span(
    ffmpeg: &str,
    file: &Path,
    start_ms: i64,
    len_ms: i64,
    bins: usize,
) -> Result<Wave> {
    let bins = bins.clamp(MIN_BINS, MAX_BINS);
    let secs = |ms: i64| format!("{:.3}", ms.max(0) as f64 / 1000.0);
    let out = tokio::process::Command::new(ffmpeg)
        .args(["-v", "error", "-nostdin", "-ss", &secs(start_ms), "-i"])
        .arg(file)
        .args(["-vn", "-ac", "1", "-ar"])
        .arg(RATE.to_string())
        .args([
            "-t",
            &secs(len_ms.min(MAX_SECS as i64 * 1000)),
            "-f",
            "s16le",
            "-",
        ])
        .kill_on_drop(true)
        .output()
        .await
        .context("couldn't run ffmpeg")?;
    if !out.status.success() {
        bail!(
            "ffmpeg couldn't read the song: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    let samples = samples_of(&out.stdout);
    if samples.is_empty() {
        bail!("the song has no sound");
    }
    Ok(Wave {
        duration_ms: samples.len() as i64 * 1000 / RATE,
        peaks: peaks(&samples, bins),
    })
}

/// How loud a stretch of sound is, as broadcast meters read it (EBU R128).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Loudness {
    /// Integrated loudness over the stretch, in LUFS (about -14 is loud music, -23 broadcast).
    pub lufs: f32,
    /// The highest true peak, in dBTP (0 is full scale).
    pub true_peak_db: f32,
    /// How much the loudness varies, in LU.
    pub range_lu: f32,
}

/// The reading in the summary ffmpeg's `ebur128` filter prints when it finishes, or `None` when there
/// is none or the stretch was silent (ffmpeg then reports no level at all).
pub fn parse_ebur128(stderr: &str) -> Option<Loudness> {
    let summary = &stderr[stderr.rfind("Summary:")?..];
    let value = |key: &str, unit: &str| -> Option<f32> {
        summary.lines().find_map(|l| {
            let l = l.trim();
            let rest = l.strip_prefix(key)?.trim().strip_suffix(unit)?.trim();
            rest.parse::<f32>().ok().filter(|v| v.is_finite())
        })
    };
    Some(Loudness {
        lufs: value("I:", "LUFS").filter(|v| *v > -69.9)?,
        true_peak_db: value("Peak:", "dBFS")?,
        range_lu: value("LRA:", "LU").unwrap_or(0.0),
    })
}

/// Measure the `len_ms` of `file`'s sound from `start_ms`. `None` for silence.
pub async fn loudness(
    ffmpeg: &str,
    file: &Path,
    start_ms: i64,
    len_ms: i64,
) -> Result<Option<Loudness>> {
    let secs = |ms: i64| format!("{:.3}", ms.max(0) as f64 / 1000.0);
    let out = tokio::process::Command::new(ffmpeg)
        .args([
            "-hide_banner",
            "-nostats",
            "-nostdin",
            "-ss",
            &secs(start_ms),
            "-t",
        ])
        .arg(secs(len_ms.min(MAX_SECS as i64 * 1000)))
        .arg("-i")
        .arg(file)
        .args(["-vn", "-af", "ebur128=peak=true", "-f", "null", "-"])
        .kill_on_drop(true)
        .output()
        .await
        .context("couldn't run ffmpeg")?;
    let err = String::from_utf8_lossy(&out.stderr);
    if !out.status.success() {
        bail!("ffmpeg couldn't measure the sound: {}", err.trim());
    }
    Ok(parse_ebur128(&err))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SUMMARY: &str = "[Parsed_ebur128_0 @ 0x1] Summary:

  Integrated loudness:
    I:         -14.9 LUFS
    Threshold: -24.9 LUFS

  Loudness range:
    LRA:         5.3 LU
    LRA low:   -18.1 LUFS

  True peak:
    Peak:       -0.4 dBFS
";

    #[test]
    fn the_summary_ffmpeg_prints_is_read() {
        let l = parse_ebur128(&format!(
            "frame info
t: 1.0 I: -30.0 LUFS
{SUMMARY}"
        ))
        .unwrap();
        assert_eq!(
            l,
            Loudness {
                lufs: -14.9,
                true_peak_db: -0.4,
                range_lu: 5.3
            }
        );
    }

    #[test]
    fn silence_and_no_summary_are_no_reading() {
        assert!(parse_ebur128("no summary here").is_none());
        let silent = SUMMARY.replace("-14.9 LUFS", "-70.0 LUFS");
        assert!(parse_ebur128(&silent).is_none());
        assert!(parse_ebur128(&SUMMARY.replace("-14.9", "-inf")).is_none());
    }

    #[test]
    fn each_bar_is_the_loudest_sample_of_its_slice_scaled_to_the_loudest() {
        let s = [100, -200, 50, 10, 0, 0, 400, -300];
        assert_eq!(peaks(&s, 4), vec![0.5, 0.125, 0.0, 1.0]);
    }

    #[test]
    fn a_quiet_song_is_scaled_up_and_silence_stays_silent() {
        assert_eq!(peaks(&[5, -10, 2, 1], 2), vec![1.0, 0.2]);
        assert_eq!(peaks(&[0; 8], 4), vec![0.0; 4]);
    }

    #[test]
    fn there_are_never_more_bars_than_samples() {
        assert_eq!(peaks(&[1, 2, 3], 10).len(), 3);
        assert!(peaks(&[], 10).is_empty());
    }

    #[test]
    fn uneven_slices_use_every_sample() {
        let mut s = vec![1i16; 7];
        s[6] = 700;
        let p = peaks(&s, 3);
        assert_eq!(p.len(), 3);
        assert_eq!(p[2], 1.0, "the last sample is in the last bar");
    }

    #[test]
    fn raw_bytes_are_little_endian_samples() {
        assert_eq!(samples_of(&[1, 0, 0xff, 0xff, 9]), vec![1, -1]);
    }

    #[tokio::test]
    async fn a_stretch_of_a_file_is_read_on_its_own() {
        let ffmpeg = std::process::Command::new("ffmpeg")
            .arg("-version")
            .output();
        if ffmpeg.is_err() {
            eprintln!("skipping: ffmpeg not found");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        // 1 s of silence, then 1 s of tone
        let file = dir.path().join("a.wav");
        let ok = std::process::Command::new("ffmpeg")
            .args(["-y", "-v", "error", "-f", "lavfi", "-i"])
            .arg("sine=frequency=440:duration=1")
            .args(["-af", "adelay=1000:all=1", "-t", "2"])
            .arg(&file)
            .status()
            .unwrap()
            .success();
        assert!(ok);
        let quiet = analyze_span("ffmpeg", &file, 0, 800, 20).await.unwrap();
        let loud = analyze_span("ffmpeg", &file, 1100, 800, 20).await.unwrap();
        assert!(quiet.peaks.iter().all(|&p| p == 0.0), "{:?}", quiet.peaks);
        assert!(loud.peaks.iter().any(|&p| p > 0.9), "{:?}", loud.peaks);
        assert!(
            (700..=900).contains(&loud.duration_ms),
            "{}",
            loud.duration_ms
        );
    }
}
