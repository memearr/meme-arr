//! Transitions between the clips of a sequence: a clip comes in over the end of the one before
//! it (a dissolve, a wipe, a slide...) instead of cutting to it.
//!
//! A transition is on the clip that comes in. The two overlap for its length, so the sequence is
//! that much shorter, and the sound of the two is cross-faded the same way. A sequence with no
//! transitions is joined as before, by a plain concatenation.

use super::*;

/// Shortest and longest a transition may be.
pub const MIN_TRANSITION_MS: i64 = 100;
pub const MAX_TRANSITION_MS: i64 = 3000;

/// How a clip comes in over the one before it (ffmpeg's `xfade` transitions).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransitionKind {
    /// A cross-fade.
    Fade,
    /// Through black.
    FadeBlack,
    /// Through white.
    FadeWhite,
    /// The new clip is pushed in by a wipe from the right, left, bottom or top.
    WipeLeft,
    WipeRight,
    WipeUp,
    WipeDown,
    /// The new clip slides in over the old one, from the right, left, bottom or top.
    SlideLeft,
    SlideRight,
    SlideUp,
    SlideDown,
    /// The new clip opens from the middle as a circle, or the old one closes to it.
    CircleOpen,
    CircleClose,
    /// A sweep around the middle.
    Radial,
    /// The picture breaks into blocks, then back.
    Pixelize,
    /// A soft dissolve.
    Dissolve,
}

impl TransitionKind {
    /// The name `xfade` knows it by.
    pub fn xfade(self) -> &'static str {
        match self {
            TransitionKind::Fade => "fade",
            TransitionKind::FadeBlack => "fadeblack",
            TransitionKind::FadeWhite => "fadewhite",
            TransitionKind::WipeLeft => "wipeleft",
            TransitionKind::WipeRight => "wiperight",
            TransitionKind::WipeUp => "wipeup",
            TransitionKind::WipeDown => "wipedown",
            TransitionKind::SlideLeft => "slideleft",
            TransitionKind::SlideRight => "slideright",
            TransitionKind::SlideUp => "slideup",
            TransitionKind::SlideDown => "slidedown",
            TransitionKind::CircleOpen => "circleopen",
            TransitionKind::CircleClose => "circleclose",
            TransitionKind::Radial => "radial",
            TransitionKind::Pixelize => "pixelize",
            TransitionKind::Dissolve => "dissolve",
        }
    }
}

/// A transition into a clip: its kind and how long it takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Transition {
    pub kind: TransitionKind,
    pub ms: i64,
}

/// Why the transitions of a sequence can't be used, or `None`. `clips` is each clip's transition
/// in and its length. The first clip has nothing to come in over, and a clip has to be long enough
/// for the transition into it and the one out of it.
pub fn validate_transitions(clips: &[(Option<Transition>, i64)]) -> Option<String> {
    for (i, (t, len)) in clips.iter().enumerate() {
        let Some(t) = t else { continue };
        let n = i + 1;
        if i == 0 {
            return Some("the first clip has no clip before it to come in over".into());
        }
        if !(MIN_TRANSITION_MS..=MAX_TRANSITION_MS).contains(&t.ms) {
            return Some(format!(
                "clip #{n}: a transition must be between {MIN_TRANSITION_MS} and {MAX_TRANSITION_MS} ms"
            ));
        }
        let out = clips
            .get(i + 1)
            .and_then(|(next, _)| next.as_ref())
            .map_or(0, |next| next.ms);
        if t.ms + out > *len {
            return Some(format!(
                "clip #{n} is too short for the transitions into and out of it"
            ));
        }
    }
    None
}

/// Whether any clip comes in with a transition.
pub fn has_transitions(transitions: &[Option<Transition>]) -> bool {
    transitions.iter().skip(1).any(Option::is_some)
}

/// Where each clip starts in the joined sequence, in ms, given each clip's length and transition in.
pub fn clip_starts(lens: &[i64], transitions: &[Option<Transition>]) -> Vec<i64> {
    let mut at = 0;
    lens.iter()
        .enumerate()
        .map(|(i, len)| {
            if let Some(t) = transitions.get(i).copied().flatten().filter(|_| i > 0) {
                at -= t.ms;
            }
            let start = at;
            at += len;
            start
        })
        .collect()
}

/// The length of the joined sequence.
pub fn joined_length(lens: &[i64], transitions: &[Option<Transition>]) -> i64 {
    let starts = clip_starts(lens, transitions);
    match (starts.last(), lens.last()) {
        (Some(s), Some(l)) => s + l,
        _ => 0,
    }
}

fn secs_of(ms: i64) -> String {
    format!("{:.3}", ms as f64 / 1000.0)
}

/// The filter graph that joins `lens.len()` parts (inputs 0, 1, ...; each with a picture and, when
/// `audio`, sound), each coming in as its transition says, to `[vout]` (and `[aout]`). The parts
/// are all the shape and frame rate of the sequence; they are made the same timebase and format here
/// so `xfade` will take them.
pub fn join_graph(
    lens: &[i64],
    transitions: &[Option<Transition>],
    fps: u32,
    audio: bool,
) -> String {
    let mut parts = Vec::new();
    for i in 0..lens.len() {
        parts.push(format!(
            "[{i}:v]fps={fps},settb=AVTB,setpts=PTS-STARTPTS,setsar=1,format=yuv420p[nv{i}]"
        ));
        if audio {
            parts.push(format!(
                "[{i}:a]aresample=48000,aformat=sample_fmts=fltp:channel_layouts=stereo,asetpts=PTS-STARTPTS[na{i}]"
            ));
        }
    }
    let last = lens.len().saturating_sub(1);
    if last == 0 {
        // one clip: nothing to join
        parts.push("[nv0]null[vout]".into());
        if audio {
            parts.push("[na0]anull[aout]".into());
        }
        return parts.join(";");
    }
    let (mut v, mut a) = ("[nv0]".to_string(), "[na0]".to_string());
    let mut acc = lens[0];
    for (k, len) in lens.iter().enumerate().skip(1) {
        // the last join is the sequence
        let (nv, na) = if k == last {
            ("[vout]".to_string(), "[aout]".to_string())
        } else {
            (format!("[v{k}]"), format!("[a{k}]"))
        };
        match transitions.get(k).copied().flatten() {
            Some(t) => {
                parts.push(format!(
                    "{v}[nv{k}]xfade=transition={}:duration={}:offset={}{nv}",
                    t.kind.xfade(),
                    secs_of(t.ms),
                    secs_of((acc - t.ms).max(0))
                ));
                if audio {
                    parts.push(format!(
                        "{a}[na{k}]acrossfade=d={}:c1=tri:c2=tri{na}",
                        secs_of(t.ms)
                    ));
                }
                acc += len - t.ms;
            }
            None => {
                if audio {
                    parts.push(format!("{v}{a}[nv{k}][na{k}]concat=n=2:v=1:a=1{nv}{na}"));
                } else {
                    parts.push(format!("{v}[nv{k}]concat=n=2:v=1:a=0{nv}"));
                }
                acc += len;
            }
        }
        v = nv;
        a = na;
    }
    parts.join(";")
}

/// The ffmpeg arguments that join `parts` (in order) into `out`, with `join_graph`. The picture is
/// made again in good quality; the sound is kept as PCM for the one lossy encode that follows.
pub fn join_args(
    parts: &[PathBuf],
    lens: &[i64],
    transitions: &[Option<Transition>],
    fps: u32,
    audio: bool,
    out: &Path,
) -> Vec<String> {
    let mut args: Vec<String> = vec!["-y".into(), "-v".into(), "error".into()];
    for p in parts {
        args.extend(["-i".to_string(), p.to_string_lossy().into_owned()]);
    }
    args.extend([
        "-filter_complex".to_string(),
        join_graph(lens, transitions, fps, audio),
        "-map".into(),
        "[vout]".into(),
    ]);
    if audio {
        args.extend(["-map".to_string(), "[aout]".into()]);
    }
    args.extend([
        "-c:v".to_string(),
        "libx264".into(),
        "-crf".into(),
        "20".into(),
        "-pix_fmt".into(),
        "yuv420p".into(),
    ]);
    args.extend(audio_args(if audio {
        AudioMode::Pcm
    } else {
        AudioMode::None
    }));
    args.extend(privacy_args(None));
    args.push(out.to_string_lossy().into_owned());
    args
}

#[cfg(test)]
#[path = "transition_tests.rs"]
mod tests;
