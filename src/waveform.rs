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
    let bins = bins.clamp(MIN_BINS, MAX_BINS);
    let out = tokio::process::Command::new(ffmpeg)
        .args(["-v", "error", "-nostdin", "-i"])
        .arg(file)
        .args(["-vn", "-ac", "1", "-ar"])
        .arg(RATE.to_string())
        .args(["-t", &MAX_SECS.to_string(), "-f", "s16le", "-"])
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
