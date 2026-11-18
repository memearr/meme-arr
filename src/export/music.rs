//! Music under a clip, and the volume of the clip's own sound.
//!
//! The sound of an export is built as an audio filter graph: the clip's own sound
//! (turned up or down) and one song (from a chosen point in it, starting at a chosen
//! moment of the clip, with a fade in and out, turned up or down) are mixed, kept
//! from clipping, and cut at the end of the clip. The song is a second input of the
//! one ffmpeg command, so nothing is decoded twice or written to a temp file.
//!
//! Only MP4 and WebM can carry sound; GIF and WebP exports refuse music.

use super::*;

/// Loudest a volume control may be: 200%.
pub const MAX_VOLUME: f32 = 2.0;
/// Furthest into a song the start point may be (6 hours: more than any song).
pub const MAX_SONG_START_MS: i64 = 6 * 3600 * 1000;

fn one() -> f32 {
    1.0
}

/// A song laid under the clip.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Soundtrack {
    /// The song's id in Navidrome (see `crate::music`).
    pub id: String,
    /// Where in the song to start playing from.
    #[serde(default)]
    pub start_ms: i64,
    /// When in the clip the song comes in, from the start of the clip.
    #[serde(default)]
    pub at_ms: i64,
    /// 0 silent .. 1 as recorded .. 2 twice as loud.
    #[serde(default = "one")]
    pub volume: f32,
    #[serde(default)]
    pub fade_in_ms: i64,
    #[serde(default)]
    pub fade_out_ms: i64,
    /// Ducking: how far the song dips while the clip's own sound is playing, 0 (off) .. 1
    /// (as far as it goes). Needs the clip's sound kept.
    #[serde(default)]
    pub duck: f32,
    /// The song's file, filled in by the server before the export runs.
    #[serde(skip)]
    pub file: Option<PathBuf>,
}

/// Why the music settings of a `clip_ms` clip can't be used, or `None` if they can.
pub fn validate_music(
    music: Option<&Soundtrack>,
    clip_volume: f32,
    clip_ms: i64,
    format: Format,
    keep_audio: bool,
) -> Option<String> {
    if !clip_volume.is_finite() || !(0.0..=MAX_VOLUME).contains(&clip_volume) {
        return Some(format!(
            "the clip's volume must be between 0 and {MAX_VOLUME}"
        ));
    }
    if (clip_volume - 1.0).abs() > f32::EPSILON && !keep_audio {
        return Some("the clip's volume only matters when its sound is kept".into());
    }
    let m = music?;
    if !format.supports_audio() {
        return Some(format!(
            "{} doesn't support audio -- choose MP4 or WebM to add music",
            format.ext().to_uppercase()
        ));
    }
    if !m.volume.is_finite() || !(0.0..=MAX_VOLUME).contains(&m.volume) {
        return Some(format!(
            "the music's volume must be between 0 and {MAX_VOLUME}"
        ));
    }
    if !(0..=MAX_SONG_START_MS).contains(&m.start_ms) {
        return Some("the music's start point is outside the song".into());
    }
    if m.at_ms < 0 || m.at_ms >= clip_ms {
        return Some("the music must come in within the clip".into());
    }
    let len = clip_ms - m.at_ms;
    if m.fade_in_ms < 0 || m.fade_out_ms < 0 || m.fade_in_ms > len || m.fade_out_ms > len {
        return Some("a fade can't be longer than the music is playing".into());
    }
    if !m.duck.is_finite() || !(0.0..=1.0).contains(&m.duck) {
        return Some("the ducking must be between 0 and 1".into());
    }
    if m.duck > 0.0 && !keep_audio {
        return Some("ducking needs the clip's sound kept (it is what the music ducks for)".into());
    }
    if !crate::music::valid_id(&m.id) {
        return Some("that isn't a song id".into());
    }
    None
}

/// The extra input and filters an export's sound needs.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioMix {
    /// Input options and the song's `-i`, to go after the clip's own input (and any
    /// layers') and before the filters.
    pub input_args: Vec<String>,
    /// Filters, ending in the labelled output `[aout]`.
    pub graph: String,
}

fn secs(ms: i64) -> String {
    format!("{:.3}", ms as f64 / 1000.0)
}

/// The mix for a `clip_ms` clip, or `None` when the sound is left as it is (no music, the
/// clip's volume untouched). `clip_audio` is whether the clip's own sound is in the
/// export (asked for, and the source has any). `music_input` is the song's input number.
pub fn plan_audio(
    music: Option<&Soundtrack>,
    clip_volume: f32,
    clip_audio: bool,
    clip_ms: i64,
    music_input: usize,
) -> Option<AudioMix> {
    let song = music.filter(|m| m.file.is_some());
    let own = clip_audio && (song.is_some() || (clip_volume - 1.0).abs() > f32::EPSILON);
    if song.is_none() && !own {
        return None;
    }
    // The song dips under the clip's own sound: it is the sidechain of a compressor on the song.
    let duck = song
        .map(|m| m.duck)
        .filter(|d| *d > 0.0 && own)
        .unwrap_or(0.0);
    let mut parts = Vec::new();
    let mut labels = Vec::new();
    let mut input_args = Vec::new();
    if own {
        let to = if duck > 0.0 { "[ca0]" } else { "[ca]" };
        parts.push(format!(
            "[0:a:0]aformat=channel_layouts=stereo,volume={clip_volume:.3}{to}"
        ));
        if duck > 0.0 {
            // one copy is mixed, the other listens (at the song's own rate)
            parts.push("[ca0]asplit=2[ca][cs0]".into());
            parts.push("[cs0]aresample=48000[sc]".into());
        }
        labels.push("[ca]");
    }
    if let Some(m) = song {
        let len = clip_ms - m.at_ms;
        input_args.extend([
            "-ss".to_string(),
            secs(m.start_ms),
            "-t".into(),
            secs(len),
            "-i".into(),
            m.file
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default(),
        ]);
        let mut chain = format!(
            "[{music_input}:a:0]aresample=48000,aformat=channel_layouts=stereo,volume={:.3}",
            m.volume
        );
        if m.fade_in_ms > 0 {
            chain.push_str(&format!(",afade=t=in:st=0:d={}", secs(m.fade_in_ms)));
        }
        if m.fade_out_ms > 0 {
            chain.push_str(&format!(
                ",afade=t=out:st={}:d={}",
                secs(len - m.fade_out_ms),
                secs(m.fade_out_ms)
            ));
        }
        if m.at_ms > 0 {
            chain.push_str(&format!(",adelay={}:all=1", m.at_ms));
        }
        if duck > 0.0 {
            chain.push_str("[mu0]");
            parts.push(chain);
            parts.push(format!(
                "[mu0][sc]sidechaincompress=threshold=0.02:ratio={:.1}:attack=40:release=600:makeup=1[mu]",
                1.0 + 19.0 * duck
            ));
        } else {
            chain.push_str("[mu]");
            parts.push(chain);
        }
        labels.push("[mu]");
    }
    let tail = format!("atrim=end={},alimiter=limit=0.95[aout]", secs(clip_ms));
    parts.push(if labels.len() == 2 {
        format!(
            "{}{}amix=inputs=2:duration=longest:normalize=0,{tail}",
            labels[0], labels[1]
        )
    } else {
        format!("{}{tail}", labels[0])
    });
    Some(AudioMix {
        input_args,
        graph: parts.join(";"),
    })
}

/// Put `mix` into an export's argv. `args` is built by `export_args` (a `-vf` chain) or
/// `export_args_with_layers` (a `-filter_complex` graph); either way the song's input goes
/// ahead of the filters, the sound graph joins them, and the output's audio is the mix.
pub fn splice_audio(mut args: Vec<String>, mix: &AudioMix) -> Vec<String> {
    if let Some(i) = args.iter().position(|a| a == "-filter_complex") {
        // a layered export: one graph, with the sound after the picture
        args[i + 1] = format!("{};{}", args[i + 1], mix.graph);
        let mut at = i;
        for a in &mix.input_args {
            args.insert(at, a.clone());
            at += 1;
        }
        let shifted = at + 2;
        match args.iter().position(|a| a == "0:a:0?") {
            Some(j) => args[j] = "[aout]".into(),
            None => {
                // no sound was mapped (the clip's own is not kept): map the mix
                let k = args[shifted..]
                    .iter()
                    .position(|a| a == "[out]")
                    .map(|p| p + shifted + 1)
                    .unwrap_or(shifted);
                args.splice(k..k, ["-map".to_string(), "[aout]".to_string()]);
            }
        }
    } else if let Some(i) = args.iter().position(|a| a == "-vf") {
        let mut add = mix.input_args.clone();
        add.extend([
            "-filter_complex".to_string(),
            mix.graph.clone(),
            "-map".into(),
            "0:v:0".into(),
            "-map".into(),
            "[aout]".into(),
        ]);
        args.splice(i..i, add);
    }
    args
}

#[cfg(test)]
#[path = "music_tests.rs"]
mod tests;
