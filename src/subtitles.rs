//! Subtitle handling: parse SRT/VTT text into timed cues, and build a trimmed
//! SRT for a clip window (shifted to start at zero) for burning at export time.
//!
//! Actual extraction of an embedded subtitle track from a media file is done by
//! ffmpeg (see [`crate::ffmpeg`]); this module turns the resulting text into the
//! cue model we index and search.

/// One subtitle line with its on-screen window, in milliseconds.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Cue {
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
}

/// Parse an SRT or WebVTT document into cues. The two formats differ mainly in
/// the timestamp separator (`,` vs `.`) and VTT's `WEBVTT` header / cue
/// settings, all of which we tolerate.
pub fn parse(input: &str) -> Vec<Cue> {
    let mut cues = Vec::new();
    // Normalise line endings and split into blocks separated by blank lines.
    let normalized = input.replace("\r\n", "\n").replace('\r', "\n");
    for block in normalized.split("\n\n") {
        if let Some(cue) = parse_block(block) {
            cues.push(cue);
        }
    }
    cues
}

fn parse_block(block: &str) -> Option<Cue> {
    let mut lines = block.lines().filter(|l| !l.trim().is_empty()).peekable();

    // Skip a leading numeric SRT index or a stray "WEBVTT" header line.
    let first = lines.peek()?;
    let first_trim = first.trim();
    if first_trim.eq_ignore_ascii_case("WEBVTT")
        || first_trim.parse::<u64>().is_ok() && !first_trim.contains("-->")
    {
        lines.next();
    }

    // The timing line.
    let timing = lines.next()?;
    let (start, end) = parse_timing(timing)?;

    // Everything after is the caption text.
    let text = lines.collect::<Vec<_>>().join("\n");
    let text = strip_tags(&text).trim().to_string();
    if text.is_empty() {
        return None;
    }

    Some(Cue {
        start_ms: start,
        end_ms: end,
        text,
    })
}

fn parse_timing(line: &str) -> Option<(i64, i64)> {
    let line = line.trim();
    let (lhs, rhs) = line.split_once("-->")?;
    let start = parse_timestamp(lhs.trim())?;
    // VTT cue settings may trail the end time (e.g. "line:90%"); take the first token.
    let end_tok = rhs.split_whitespace().next()?;
    let end = parse_timestamp(end_tok)?;
    Some((start, end))
}

/// Parse `HH:MM:SS,mmm`, `HH:MM:SS.mmm`, or `MM:SS.mmm` into milliseconds.
fn parse_timestamp(ts: &str) -> Option<i64> {
    let ts = ts.replace(',', ".");
    let (hms, millis) = match ts.split_once('.') {
        Some((h, m)) => (h, m),
        None => (ts.as_str(), "0"),
    };
    let parts: Vec<&str> = hms.split(':').collect();
    let (h, m, s) = match parts.as_slice() {
        [h, m, s] => (
            h.parse::<i64>().ok()?,
            m.parse::<i64>().ok()?,
            s.parse::<i64>().ok()?,
        ),
        [m, s] => (0, m.parse::<i64>().ok()?, s.parse::<i64>().ok()?),
        _ => return None,
    };
    // Pad/truncate fractional part to exactly 3 digits of milliseconds.
    let mut frac = millis
        .chars()
        .filter(|c| c.is_ascii_digit())
        .collect::<String>();
    while frac.len() < 3 {
        frac.push('0');
    }
    frac.truncate(3);
    let ms: i64 = frac.parse().ok()?;
    Some(((h * 3600 + m * 60 + s) * 1000) + ms)
}

/// Remove ASS/SSA style overrides (`{\an8}`) and HTML-ish tags (`<i>`) that
/// shouldn't be searched or displayed as text.
fn strip_tags(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut depth_angle = 0;
    let mut depth_brace = 0;
    for ch in text.chars() {
        match ch {
            '<' => depth_angle += 1,
            '>' if depth_angle > 0 => depth_angle -= 1,
            '{' => depth_brace += 1,
            '}' if depth_brace > 0 => depth_brace -= 1,
            _ if depth_angle == 0 && depth_brace == 0 => out.push(ch),
            _ => {}
        }
    }
    out
}

fn fmt_srt_ts(ms: i64) -> String {
    let ms = ms.max(0);
    let h = ms / 3_600_000;
    let m = (ms % 3_600_000) / 60_000;
    let s = (ms % 60_000) / 1000;
    let millis = ms % 1000;
    format!("{h:02}:{m:02}:{s:02},{millis:03}")
}

/// Build an SRT document containing only the cues overlapping `[start_ms, end_ms]`,
/// with all timestamps shifted so the clip begins at 0. This is what we feed to
/// ffmpeg's `subtitles` filter so burned captions line up with the trimmed clip.
///
/// `offset_ms` corrects sync drift between the indexed subtitle timing and this
/// particular video release: positive delays the subtitles (they appear later),
/// negative advances them (they appear earlier) — the same convention players
/// like mpv/VLC use for "subtitle delay". It's applied to each cue before
/// windowing, so a cue shifted into or out of `[start_ms, end_ms]` is included
/// or excluded correctly.
pub fn trimmed_srt(cues: &[Cue], start_ms: i64, end_ms: i64, offset_ms: i64) -> String {
    let mut out = String::new();
    let mut idx = 1;
    for cue in cues {
        let cue_start = cue.start_ms + offset_ms;
        let cue_end = cue.end_ms + offset_ms;
        if cue_end <= start_ms || cue_start >= end_ms {
            continue;
        }
        let s = cue_start.max(start_ms) - start_ms;
        let e = cue_end.min(end_ms) - start_ms;
        out.push_str(&format!(
            "{idx}\n{} --> {}\n{}\n\n",
            fmt_srt_ts(s),
            fmt_srt_ts(e),
            cue.text
        ));
        idx += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRT: &str = "1\n00:00:01,000 --> 00:00:02,500\nI am the one who knocks.\n\n2\n00:00:03,000 --> 00:00:04,000\n<i>Second line</i>\n";
    const VTT: &str = "WEBVTT\n\n00:00:01.000 --> 00:00:02.500 line:90%\nHello there\n\n00:01:05.250 --> 00:01:06.000\nGeneral Kenobi\n";

    #[test]
    fn parses_srt() {
        let cues = parse(SRT);
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[0].start_ms, 1000);
        assert_eq!(cues[0].end_ms, 2500);
        assert_eq!(cues[0].text, "I am the one who knocks.");
        assert_eq!(cues[1].text, "Second line"); // tag stripped
    }

    #[test]
    fn parses_vtt_with_cue_settings() {
        let cues = parse(VTT);
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[0].start_ms, 1000);
        assert_eq!(cues[0].end_ms, 2500);
        assert_eq!(cues[1].start_ms, 65_250);
    }

    #[test]
    fn parses_mm_ss_timestamp() {
        assert_eq!(parse_timestamp("05:02.250"), Some(302_250));
    }

    #[test]
    fn strips_ass_overrides() {
        assert_eq!(strip_tags("{\\an8}Top text"), "Top text");
    }

    #[test]
    fn trimmed_srt_shifts_and_clips() {
        let cues = parse(SRT);
        // Window 1.2s .. 3.5s should include both cues, clipped and shifted.
        let out = trimmed_srt(&cues, 1200, 3500, 0);
        assert!(out.contains("00:00:00,000 --> 00:00:01,300")); // cue1 clipped at front
        assert!(out.contains("I am the one who knocks."));
        assert!(out.contains("Second line"));
        // cue2 started at 3000ms -> 1800ms after shift
        assert!(out.contains("00:00:01,800 --> 00:00:02,300"));
    }

    #[test]
    fn trimmed_srt_excludes_out_of_window() {
        let cues = parse(SRT);
        let out = trimmed_srt(&cues, 10_000, 20_000, 0);
        assert!(out.trim().is_empty());
    }

    #[test]
    fn trimmed_srt_positive_offset_delays_subtitles() {
        let cues = parse(SRT);
        // cue1 is 1000..2500 natively; +500ms offset -> 1500..3000.
        // Window 0..5000 unshifted by offset, so result timestamps shift later.
        let out = trimmed_srt(&cues, 0, 5000, 500);
        assert!(out.contains("00:00:01,500 --> 00:00:03,000"));
        assert!(out.contains("I am the one who knocks."));
    }

    #[test]
    fn trimmed_srt_negative_offset_advances_subtitles() {
        let cues = parse(SRT);
        // cue1 is 1000..2500 natively; -500ms offset -> 500..2000.
        let out = trimmed_srt(&cues, 0, 5000, -500);
        assert!(out.contains("00:00:00,500 --> 00:00:02,000"));
    }

    #[test]
    fn trimmed_srt_offset_can_pull_a_cue_into_or_out_of_window() {
        let cues = parse(SRT);
        // cue2 is natively 3000..4000, just outside a 0..3000 window. A -500ms
        // offset (advance) pulls it in; clipped at the window edge for display.
        let out = trimmed_srt(&cues, 0, 3000, -500);
        assert!(out.contains("Second line"));

        // Conversely a +1000ms offset pushes cue1 (1000..2500) out of a
        // 0..1000 window entirely.
        let out2 = trimmed_srt(&cues, 0, 1000, 1000);
        assert!(out2.trim().is_empty());
    }

    #[test]
    fn trimmed_srt_offset_never_produces_negative_timestamp() {
        let cues = parse(SRT);
        // cue1 is natively 1000..2500; a -1500ms offset shifts it to -500..1000,
        // straddling the start of the window. It must clamp to 0, not underflow.
        let out = trimmed_srt(&cues, 0, 5000, -1500);
        assert!(out.contains("00:00:00,000 --> 00:00:01,000"));
    }
}
