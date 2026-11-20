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

/// Where a sound sits between the speakers, -1 (all left) .. 0 (as it is) .. 1 (all right), and how
/// loud the whole mix is.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Mixdown {
    /// The clip's own sound.
    #[serde(default)]
    pub clip_pan: f32,
    /// The whole mix, 0..=2 (the clip's sound and the song together, before the limiter).
    #[serde(default = "one")]
    pub master: f32,
}

impl Default for Mixdown {
    fn default() -> Self {
        Self {
            clip_pan: 0.0,
            master: 1.0,
        }
    }
}

impl Mixdown {
    /// Whether this leaves the mix as it is.
    pub fn is_plain(&self) -> bool {
        *self == Self::default()
    }
}

/// Why a mixdown can't be used, or `None`. Panning the clip's sound needs it kept.
pub fn validate_mixdown(m: &Mixdown, keep_audio: bool) -> Option<String> {
    if !m.clip_pan.is_finite() || !(-1.0..=1.0).contains(&m.clip_pan) {
        return Some("the clip's pan must be between -1 (left) and 1 (right)".into());
    }
    if m.clip_pan != 0.0 && !keep_audio {
        return Some("the clip's pan only matters when its sound is kept".into());
    }
    if !m.master.is_finite() || !(0.0..=MAX_VOLUME).contains(&m.master) {
        return Some(format!(
            "the master volume must be between 0 and {MAX_VOLUME}"
        ));
    }
    None
}

/// The `pan` filter that places a stereo sound at `pan` (-1 left .. 1 right), or "" for the middle.
/// It is the Web Audio stereo panner's law, so the player's pan and the export's agree: moving a
/// sound toward a side folds the other channel into it as it goes, and the sound is never louder.
pub fn pan_filter(pan: f32) -> String {
    if !pan.is_finite() || pan.abs() < 0.0005 {
        return String::new();
    }
    let p = pan.clamp(-1.0, 1.0) as f64;
    let x = if p <= 0.0 { p + 1.0 } else { p };
    let (gl, gr) = (
        (x * std::f64::consts::FRAC_PI_2).cos(),
        (x * std::f64::consts::FRAC_PI_2).sin(),
    );
    if p <= 0.0 {
        format!("pan=stereo|c0=c0+{gl:.4}*c1|c1={gr:.4}*c1")
    } else {
        format!("pan=stereo|c0={gl:.4}*c0|c1=c1+{gr:.4}*c0")
    }
}

/// Most points a volume line may have.
pub const MAX_CURVE_POINTS: usize = 64;

/// One point of a volume line: at `t_ms` into the clip the sound is at `gain` times its volume
/// (0 silent .. 1 as set). Between points the volume moves in a straight line; before the first
/// point it is the first one's, after the last it is the last's.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct VolumePoint {
    pub t_ms: i64,
    pub gain: f32,
}

/// Why a volume line over a `clip_ms` clip can't be used, or `None`. `what` names it.
pub fn validate_curve(points: &[VolumePoint], clip_ms: i64, what: &str) -> Option<String> {
    if points.len() > MAX_CURVE_POINTS {
        return Some(format!("{what} has more than {MAX_CURVE_POINTS} points"));
    }
    for (i, p) in points.iter().enumerate() {
        if !(0..=clip_ms).contains(&p.t_ms) {
            return Some(format!("{what} has a point outside the clip"));
        }
        if !p.gain.is_finite() || !(0.0..=1.0).contains(&p.gain) {
            return Some(format!("{what} has a point that isn't between 0 and 1"));
        }
        if i > 0 && p.t_ms <= points[i - 1].t_ms {
            return Some(format!(
                "{what} must have its points in time order, one per moment"
            ));
        }
    }
    None
}

/// Why the volume line of the clip's own sound can't be used, or `None`.
pub fn validate_clip_curve(
    points: &[VolumePoint],
    clip_ms: i64,
    keep_audio: bool,
) -> Option<String> {
    if !points.is_empty() && !keep_audio {
        return Some("the clip's volume line only matters when its sound is kept".into());
    }
    validate_curve(points, clip_ms, "the clip's volume line")
}

/// An ffmpeg `volume` filter that follows `points` (times from the start of the sound it is on,
/// minus `shift_ms`: a song that comes in at `shift_ms` of the clip has its own clock). Empty when
/// there are no points.
pub fn curve_filter(points: &[VolumePoint], shift_ms: i64) -> String {
    let Some(last) = points.last() else {
        return String::new();
    };
    let t = |p: &VolumePoint| (p.t_ms - shift_ms) as f64 / 1000.0;
    let mut e = format!("{:.3}", last.gain);
    for w in points.windows(2).rev() {
        let (a, b) = (&w[0], &w[1]);
        e = format!(
            "if(lt(t,{tb:.3}),{ga:.3}+({gb:.3}-{ga:.3})*(t-{ta:.3})/{span:.3},{e})",
            tb = t(b),
            ta = t(a),
            ga = a.gain,
            gb = b.gain,
            span = t(b) - t(a),
        );
    }
    let first = &points[0];
    format!(
        "volume=eval=frame:volume='if(lt(t,{:.3}),{:.3},{e})'",
        t(first),
        first.gain
    )
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
    /// How loud the song is over the clip, as a line (see [`VolumePoint`]); it multiplies
    /// `volume`. Empty: steady.
    #[serde(default)]
    pub curve: Vec<VolumePoint>,
    /// Where the song sits between the speakers, -1 (left) .. 1 (right); 0 leaves it be.
    #[serde(default)]
    pub pan: f32,
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
    if !m.pan.is_finite() || !(-1.0..=1.0).contains(&m.pan) {
        return Some("the music's pan must be between -1 (left) and 1 (right)".into());
    }
    if !m.duck.is_finite() || !(0.0..=1.0).contains(&m.duck) {
        return Some("the ducking must be between 0 and 1".into());
    }
    if m.duck > 0.0 && !keep_audio {
        return Some("ducking needs the clip's sound kept (it is what the music ducks for)".into());
    }
    if let Some(why) = validate_curve(&m.curve, clip_ms, "the music's volume line") {
        return Some(why);
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

/// One song in a mix: when it comes in over the whole export and how long it plays.
#[derive(Debug, Clone, Copy)]
pub struct Placed<'a> {
    pub music: &'a Soundtrack,
    /// From the start of the export.
    pub at_ms: i64,
    /// How long it plays (cut there, whatever the song's length).
    pub len_ms: i64,
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
    let songs: Vec<Placed> = music
        .filter(|m| m.file.is_some())
        .map(|m| Placed {
            music: m,
            at_ms: m.at_ms,
            len_ms: clip_ms - m.at_ms,
        })
        .into_iter()
        .collect();
    plan_mix(&songs, clip_volume, clip_audio, clip_ms, music_input)
}

/// `plan_audio` for a single export, with a volume line on the clip's own sound.
pub fn plan_audio_curved(
    music: Option<&Soundtrack>,
    clip_volume: f32,
    clip_curve: &[VolumePoint],
    clip_audio: bool,
    clip_ms: i64,
    music_input: usize,
) -> Option<AudioMix> {
    plan_audio_mixed(
        music,
        clip_volume,
        clip_curve,
        &Mixdown::default(),
        clip_audio,
        clip_ms,
        music_input,
    )
}

/// `plan_audio_curved`, with the clip's pan and the master volume.
pub fn plan_audio_mixed(
    music: Option<&Soundtrack>,
    clip_volume: f32,
    clip_curve: &[VolumePoint],
    mixdown: &Mixdown,
    clip_audio: bool,
    clip_ms: i64,
    music_input: usize,
) -> Option<AudioMix> {
    let songs: Vec<Placed> = music
        .filter(|m| m.file.is_some())
        .map(|m| Placed {
            music: m,
            at_ms: m.at_ms,
            len_ms: clip_ms - m.at_ms,
        })
        .into_iter()
        .collect();
    plan_mix_mixed(
        &songs,
        clip_volume,
        clip_curve,
        mixdown,
        clip_audio,
        clip_ms,
        music_input,
    )
}

/// The mix for `total_ms` of sound: the export's own sound (turned up or down) and any number
/// of songs, each placed and cut as `Placed` says. The songs are inputs `first_input`,
/// `first_input + 1`, ... in order. A song with ducking dips while the export's own sound plays.
pub fn plan_mix(
    songs: &[Placed],
    clip_volume: f32,
    clip_audio: bool,
    total_ms: i64,
    first_input: usize,
) -> Option<AudioMix> {
    plan_mix_curved(songs, clip_volume, &[], clip_audio, total_ms, first_input)
}

/// `plan_mix`, with a volume line on the export's own sound too (`clip_curve`, times from the
/// start of the export).
pub fn plan_mix_curved(
    songs: &[Placed],
    clip_volume: f32,
    clip_curve: &[VolumePoint],
    clip_audio: bool,
    total_ms: i64,
    first_input: usize,
) -> Option<AudioMix> {
    plan_mix_mixed(
        songs,
        clip_volume,
        clip_curve,
        &Mixdown::default(),
        clip_audio,
        total_ms,
        first_input,
    )
}

/// `plan_mix_curved`, with the export's own sound panned and the whole mix at the master volume.
pub fn plan_mix_mixed(
    songs: &[Placed],
    clip_volume: f32,
    clip_curve: &[VolumePoint],
    mixdown: &Mixdown,
    clip_audio: bool,
    total_ms: i64,
    first_input: usize,
) -> Option<AudioMix> {
    let songs: Vec<&Placed> = songs.iter().filter(|p| p.music.file.is_some()).collect();
    let own = clip_audio
        && (!songs.is_empty()
            || !clip_curve.is_empty()
            || !mixdown.is_plain()
            || (clip_volume - 1.0).abs() > f32::EPSILON);
    if songs.is_empty() && !own {
        return None;
    }
    let single = songs.len() == 1;
    // Which songs dip under the export's own sound: it is the sidechain of a compressor on each.
    let ducked: Vec<usize> = if own {
        (0..songs.len())
            .filter(|&i| songs[i].music.duck > 0.0)
            .collect()
    } else {
        vec![]
    };
    let mut parts = Vec::new();
    let mut labels: Vec<String> = Vec::new();
    let mut input_args = Vec::new();
    if own {
        let to = if ducked.is_empty() { "[ca]" } else { "[ca0]" };
        let line = curve_filter(clip_curve, 0);
        let line = if line.is_empty() {
            line
        } else {
            format!(",{line}")
        };
        let pan = pan_filter(mixdown.clip_pan);
        let pan = if pan.is_empty() {
            pan
        } else {
            format!(",{pan}")
        };
        parts.push(format!(
            "[0:a:0]aformat=channel_layouts=stereo{pan},volume={clip_volume:.3}{line}{to}"
        ));
        if !ducked.is_empty() {
            // one copy is mixed, the others listen (each at the songs' own rate)
            let name = |i: usize| {
                if single {
                    "cs0".to_string()
                } else {
                    format!("cs{i}")
                }
            };
            let outs: String = ducked.iter().map(|&i| format!("[{}]", name(i))).collect();
            parts.push(format!("[ca0]asplit={}[ca]{outs}", ducked.len() + 1));
            for &i in &ducked {
                let sc = if single {
                    "sc".to_string()
                } else {
                    format!("sc{i}")
                };
                parts.push(format!("[{}]aresample=48000[{sc}]", name(i)));
            }
        }
        labels.push("[ca]".into());
    }
    for (i, p) in songs.iter().enumerate() {
        let m = p.music;
        input_args.extend([
            "-ss".to_string(),
            secs(m.start_ms),
            "-t".into(),
            secs(p.len_ms),
            "-i".into(),
            m.file
                .as_ref()
                .map(|f| f.to_string_lossy().into_owned())
                .unwrap_or_default(),
        ]);
        let song_pan = pan_filter(m.pan);
        let song_pan = if song_pan.is_empty() {
            song_pan
        } else {
            format!(",{song_pan}")
        };
        let mut chain = format!(
            "[{}:a:0]aresample=48000,aformat=channel_layouts=stereo{song_pan},volume={:.3}",
            first_input + i,
            m.volume
        );
        if !m.curve.is_empty() {
            // the song's own clock starts when it comes in
            chain.push_str(&format!(",{}", curve_filter(&m.curve, m.at_ms)));
        }
        if m.fade_in_ms > 0 {
            chain.push_str(&format!(",afade=t=in:st=0:d={}", secs(m.fade_in_ms)));
        }
        if m.fade_out_ms > 0 {
            chain.push_str(&format!(
                ",afade=t=out:st={}:d={}",
                secs(p.len_ms - m.fade_out_ms),
                secs(m.fade_out_ms)
            ));
        }
        if p.at_ms > 0 {
            chain.push_str(&format!(",adelay={}:all=1", p.at_ms));
        }
        let out = if single {
            "mu".to_string()
        } else {
            format!("mu{i}")
        };
        if ducked.contains(&i) {
            let (pre, sc) = if single {
                ("mu0".to_string(), "sc".to_string())
            } else {
                (format!("mu{i}p"), format!("sc{i}"))
            };
            chain.push_str(&format!("[{pre}]"));
            parts.push(chain);
            parts.push(format!(
                "[{pre}][{sc}]sidechaincompress=threshold=0.02:ratio={:.1}:attack=40:release=600:makeup=1[{out}]",
                1.0 + 19.0 * m.duck
            ));
        } else {
            chain.push_str(&format!("[{out}]"));
            parts.push(chain);
        }
        labels.push(format!("[{out}]"));
    }
    let master = if (mixdown.master - 1.0).abs() > f32::EPSILON {
        format!("volume={:.3},", mixdown.master)
    } else {
        String::new()
    };
    let tail = format!(
        "{master}atrim=end={},alimiter=limit=0.95[aout]",
        secs(total_ms)
    );
    parts.push(if labels.len() == 1 {
        format!("{}{tail}", labels[0])
    } else {
        format!(
            "{}amix=inputs={}:duration=longest:normalize=0,{tail}",
            labels.concat(),
            labels.len()
        )
    });
    Some(AudioMix {
        input_args,
        graph: parts.join(";"),
    })
}

/// Why the songs on the clips of a sequence can't be used, or `None`. Each clip's song is
/// checked as a single clip's would be (times from the start of that clip); the clip's own
/// volume is the sequence's, checked separately.
pub fn validate_clip_music(
    clips: &[(Option<&Soundtrack>, i64)],
    format: Format,
    keep_audio: bool,
) -> Option<String> {
    clips.iter().enumerate().find_map(|(i, (m, len))| {
        validate_music(*m, 1.0, *len, format, keep_audio)
            .map(|why| format!("clip #{}: {why}", i + 1))
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
