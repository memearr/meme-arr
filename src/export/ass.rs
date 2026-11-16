//! Styled burned text: the subtitle lines of a clip written as an ASS script, so the
//! text can have a colour and outline of its own, sit anywhere in the picture, grow
//! word by word as it is spoken, and come in with a small animation.
//!
//! Plain burned subtitles (no [`TextLook`]) still go through an SRT and the
//! `subtitles` filter; only a caption that asks for a look comes through here.
//!
//! The script's coordinates are libass's usual 288-line space (`PLAY_RES`), the same
//! one the plain subtitles use, so a size means the same thing in both and the
//! editor's preview can scale text with one rule.

use super::*;

/// The script's coordinate space: a 16:9 picture 512 x 288 units.
pub const PLAY_RES: (u32, u32) = (512, 288);

/// How big the normal text is, before a preset or the size slider changes it
/// (libass's own default for plain subtitles).
const BASE_SIZE: u32 = 16;

/// How much larger a `*starred*` word is drawn.
const EMPHASIS: f32 = 1.5;

/// The share of a line's time over which its words appear, one after another. The
/// last word is up a little before the line ends, so it can be read.
const REVEAL_SPAN: f32 = 0.8;

/// How long one word (or the whole line) takes to come in.
const ENTRANCE_MS: i64 = 180;

/// How far (in script units) a sliding line travels.
const SLIDE_DISTANCE: f32 = 24.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextAnim {
    /// Appears at once.
    #[default]
    None,
    /// Fades in.
    Fade,
    /// Grows from a smaller size to full size.
    Pop,
    /// The line rises into place.
    SlideUp,
    /// The line slides in from the right.
    SlideLeft,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextReveal {
    /// The whole line is there from the start.
    #[default]
    None,
    /// Each word appears as it is (roughly) said.
    Word,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextAlign {
    Left,
    #[default]
    Center,
    Right,
}

/// How a burned caption looks, beyond its font and size. Every field is optional:
/// what is not given comes from the preset, and what the preset leaves open is the
/// plain white-with-a-black-outline caption at the bottom of the picture.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TextLook {
    /// A named starting point, see [`PRESETS`].
    #[serde(default)]
    pub preset: Option<String>,
    /// Text colour, `#RRGGBB`.
    #[serde(default)]
    pub color: Option<String>,
    /// Outline colour, `#RRGGBB`.
    #[serde(default)]
    pub outline_color: Option<String>,
    /// Outline thickness in script units (0 is none).
    #[serde(default)]
    pub outline: Option<f32>,
    /// Where the text sits, as a fraction of the picture (0 left .. 1 right).
    /// `None` keeps the usual place for the alignment.
    #[serde(default)]
    pub x: Option<f32>,
    /// Where the text sits, as a fraction of the picture (0 top .. 1 bottom). When
    /// given, the text is centred on this height; without it, it rests on the bottom.
    #[serde(default)]
    pub y: Option<f32>,
    #[serde(default)]
    pub align: TextAlign,
    #[serde(default)]
    pub anim: TextAnim,
    #[serde(default)]
    pub reveal: TextReveal,
}

/// The values a preset (or a look) settles on.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Resolved {
    color: [u8; 3],
    outline_color: [u8; 3],
    outline: f32,
    shadow: f32,
    bold: bool,
    /// An opaque box behind the text instead of an outline.
    boxed: bool,
    size: u32,
}

const WHITE: [u8; 3] = [255, 255, 255];
const BLACK: [u8; 3] = [0, 0, 0];

/// The named presets, in the order the editor lists them: `(id, label, look)`.
pub const PRESETS: &[(&str, &str)] = &[
    ("classic", "Classic (white, outlined)"),
    ("block", "Block (big yellow)"),
    ("neon", "Neon (magenta)"),
    ("horror", "Horror (blood red)"),
    ("soft", "Soft (cream, shadow)"),
    ("box", "Box (on a dark bar)"),
];

fn preset(id: &str) -> Resolved {
    match id {
        "block" => Resolved {
            color: [255, 230, 0],
            outline_color: BLACK,
            outline: 3.0,
            shadow: 0.0,
            bold: true,
            boxed: false,
            size: 36,
        },
        "neon" => Resolved {
            color: [255, 34, 255],
            outline_color: [40, 0, 60],
            outline: 1.5,
            shadow: 0.0,
            bold: true,
            boxed: false,
            size: 30,
        },
        "horror" => Resolved {
            color: [216, 27, 15],
            outline_color: [20, 0, 0],
            outline: 2.0,
            shadow: 1.0,
            bold: true,
            boxed: false,
            size: 32,
        },
        "soft" => Resolved {
            color: [240, 221, 192],
            outline_color: BLACK,
            outline: 0.0,
            shadow: 2.0,
            bold: true,
            boxed: false,
            size: 20,
        },
        "box" => Resolved {
            color: WHITE,
            outline_color: BLACK,
            outline: 3.0,
            shadow: 0.0,
            bold: false,
            boxed: true,
            size: 18,
        },
        // "classic", and anything not recognised.
        _ => Resolved {
            color: WHITE,
            outline_color: BLACK,
            outline: 2.0,
            shadow: 0.0,
            bold: false,
            boxed: false,
            size: BASE_SIZE,
        },
    }
}

/// `#RRGGBB` (the `#` is optional) as its three bytes.
pub fn parse_hex_color(s: &str) -> Option<[u8; 3]> {
    let h = s.trim().trim_start_matches('#');
    if h.len() != 6 || !h.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let n = u32::from_str_radix(h, 16).ok()?;
    Some([(n >> 16) as u8, (n >> 8) as u8, n as u8])
}

impl TextLook {
    fn resolve(&self) -> Resolved {
        let mut r = preset(self.preset.as_deref().unwrap_or("classic"));
        if let Some(c) = self.color.as_deref().and_then(parse_hex_color) {
            r.color = c;
        }
        if let Some(c) = self.outline_color.as_deref().and_then(parse_hex_color) {
            r.outline_color = c;
        }
        if let Some(o) = self.outline.filter(|o| o.is_finite()) {
            r.outline = o.clamp(0.0, 8.0);
        }
        r
    }

    /// The size a caption starts from with this look, in script units, before the
    /// size slider's percentage.
    pub fn base_size(&self) -> u32 {
        self.resolve().size
    }
}

/// `&HAABBGGRR` as a style line wants a colour.
fn style_colour(rgb: [u8; 3], alpha: u8) -> String {
    format!("&H{alpha:02X}{:02X}{:02X}{:02X}", rgb[2], rgb[1], rgb[0])
}

/// `H:MM:SS.cc`.
fn ass_time(ms: i64) -> String {
    let cs = (ms.max(0) + 5) / 10;
    format!(
        "{}:{:02}:{:02}.{:02}",
        cs / 360_000,
        cs / 6000 % 60,
        cs / 100 % 60,
        cs % 100
    )
}

/// One word of a line and whether it was starred (`*word*`).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Word {
    text: String,
    emphasis: bool,
}

/// The words of each line of `text`. A `*` switches emphasis on or off (so
/// `*two words*` emphasises both) and is not shown.
fn parse_words(text: &str) -> Vec<Vec<Word>> {
    let mut lines = Vec::new();
    let mut emph = false;
    for raw in text.lines() {
        let mut words: Vec<Word> = Vec::new();
        let mut cur = String::new();
        let mut cur_emph = false;
        let flush = |cur: &mut String, cur_emph: bool, words: &mut Vec<Word>| {
            if !cur.is_empty() {
                words.push(Word {
                    text: std::mem::take(cur),
                    emphasis: cur_emph,
                });
            }
        };
        for ch in raw.chars() {
            if ch == '*' {
                emph = !emph;
                // A star in the middle of a word starts a new run of it.
                flush(&mut cur, cur_emph, &mut words);
            } else if ch.is_whitespace() {
                flush(&mut cur, cur_emph, &mut words);
            } else {
                if cur.is_empty() {
                    cur_emph = emph;
                }
                cur.push(ch);
            }
        }
        flush(&mut cur, cur_emph, &mut words);
        if !words.is_empty() {
            lines.push(words);
        }
    }
    lines
}

/// Text that is safe inside an ASS line: no override blocks or escapes of its own.
fn ass_text(s: &str) -> String {
    s.replace('\\', "/").replace('{', "(").replace('}', ")")
}

/// Where a line is anchored: the `\an` number and the position in script units.
fn anchor(look: &TextLook) -> (u8, f32, f32) {
    let (w, h) = (PLAY_RES.0 as f32, PLAY_RES.1 as f32);
    let col = match look.align {
        TextAlign::Left => 0,
        TextAlign::Center => 1,
        TextAlign::Right => 2,
    };
    let default_x = [w * 0.05, w * 0.5, w * 0.95][col];
    let x = look
        .x
        .filter(|v| v.is_finite())
        .map_or(default_x, |v| v.clamp(0.0, 1.0) * w);
    match look.y.filter(|v| v.is_finite()) {
        // Centred on the given height: row 4-6 of the keypad.
        Some(y) => (4 + col as u8, x, y.clamp(0.0, 1.0) * h),
        // Resting on the bottom, as plain subtitles do.
        None => (1 + col as u8, x, h - 10.0),
    }
}

/// The font name to give the script. Libass finds it by family in the fonts
/// folder; one that would break the style line is not used.
fn style_font(family: Option<&str>) -> &str {
    family
        .filter(|f| !f.is_empty() && !f.contains([',', '\'', '\\', ':', '=', '\n']))
        .unwrap_or("Arial")
}

fn header(look: &TextLook, family: Option<&str>, scale_pct: u32) -> String {
    let r = look.resolve();
    let size = scaled_size(r.size, scale_pct);
    let (an, _, _) = anchor(look);
    let (border_style, outline_colour, back_colour) = if r.boxed {
        (
            3,
            style_colour(r.outline_color, 0x60),
            style_colour(BLACK, 0xA0),
        )
    } else {
        (
            1,
            style_colour(r.outline_color, 0),
            style_colour(BLACK, 0x60),
        )
    };
    // A box is drawn by the outline, so it needs some thickness to show.
    let outline = if r.boxed {
        r.outline.max(2.0)
    } else {
        r.outline
    };
    format!(
        "[Script Info]\nScriptType: v4.00+\nPlayResX: {}\nPlayResY: {}\nWrapStyle: 0\n\
         ScaledBorderAndShadow: yes\n\n[V4+ Styles]\n\
         Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, \
         BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, \
         BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\n\
         Style: Default,{},{size},{},{},{outline_colour},{back_colour},{},0,0,0,100,100,0,0,\
         {border_style},{outline:.1},{:.1},{an},16,16,10,1\n\n[Events]\n\
         Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n",
        PLAY_RES.0,
        PLAY_RES.1,
        style_font(family),
        style_colour(r.color, 0),
        style_colour(r.color, 0),
        if r.bold { -1 } else { 0 },
        r.shadow,
    )
}

/// The ASS script for the lines of `cues` that fall in `start_ms..end_ms`, timed from
/// the start of that window and moved by `offset_ms`, drawn in `look`. `family` is
/// the chosen font's family name; `scale_pct` the size slider. `None` when no line
/// falls in the window (an empty script would make ffmpeg fail).
pub fn build_ass(
    cues: &[Cue],
    window: (i64, i64),
    offset_ms: i64,
    look: &TextLook,
    family: Option<&str>,
    scale_pct: u32,
) -> Option<String> {
    let (win_start, win_end) = window;
    let r = look.resolve();
    let size = scaled_size(r.size, scale_pct);
    let big = ((size as f32) * EMPHASIS).round() as u32;
    let (an, ax, ay) = anchor(look);
    let mut out = String::new();
    let mut events = 0;

    for cue in cues {
        let cs = cue.start_ms + offset_ms;
        let ce = cue.end_ms + offset_ms;
        if ce <= win_start || cs >= win_end {
            continue;
        }
        let lines = parse_words(&cue.text);
        if lines.is_empty() {
            continue;
        }
        // On screen from here, in the window's own time.
        let from = cs.max(win_start);
        let until = ce.min(win_end);
        // Animations belong to a line that starts inside the window; one already
        // under way when the clip begins is simply there.
        let starts_here = cs >= win_start;
        let mut tags = format!("\\an{an}");
        let (px, py) = (ax, ay);
        match look.anim {
            TextAnim::SlideUp if starts_here => {
                tags.push_str(&format!(
                    "\\move({px:.1},{:.1},{px:.1},{py:.1},0,{ENTRANCE_MS})",
                    py + SLIDE_DISTANCE
                ));
            }
            TextAnim::SlideLeft if starts_here => {
                tags.push_str(&format!(
                    "\\move({:.1},{py:.1},{px:.1},{py:.1},0,{ENTRANCE_MS})",
                    px + SLIDE_DISTANCE * 3.0
                ));
            }
            _ => tags.push_str(&format!("\\pos({px:.1},{py:.1})")),
        }
        let reveal = look.reveal == TextReveal::Word;
        // Whole-line entrances (word reveal animates each word instead).
        if !reveal && starts_here {
            match look.anim {
                TextAnim::Fade => tags.push_str(&format!("\\fad({ENTRANCE_MS},0)")),
                TextAnim::Pop => tags.push_str(&format!(
                    "\\fscx70\\fscy70\\t(0,{ENTRANCE_MS},\\fscx100\\fscy100)"
                )),
                _ => {}
            }
        }

        let total_chars: usize = lines
            .iter()
            .flatten()
            .map(|w| w.text.chars().count())
            .sum::<usize>()
            .max(1);
        let dur = (ce - cs).max(1) as f32;
        let mut seen = 0usize;
        let mut body = String::new();
        let mut cur_big = false;
        for (li, line) in lines.iter().enumerate() {
            if li > 0 {
                body.push_str("\\N");
            }
            for (wi, w) in line.iter().enumerate() {
                let mut block = String::new();
                if w.emphasis != cur_big {
                    block.push_str(&format!("\\fs{}", if w.emphasis { big } else { size }));
                    cur_big = w.emphasis;
                }
                if reveal {
                    // When this word is said, in the event's own time.
                    let said = cs as f32 + dur * REVEAL_SPAN * (seen as f32 / total_chars as f32);
                    let rel = said.round() as i64 - from;
                    if rel > 0 {
                        block.push_str("\\alpha&HFF&");
                        let end = rel + ENTRANCE_MS;
                        match look.anim {
                            TextAnim::Pop => block.push_str(&format!(
                                "\\fscx60\\fscy60\\t({rel},{end},\\alpha&H00&\\fscx100\\fscy100)"
                            )),
                            TextAnim::Fade => {
                                block.push_str(&format!("\\t({rel},{end},\\alpha&H00&)"))
                            }
                            _ => block.push_str(&format!("\\t({rel},{},\\alpha&H00&)", rel + 1)),
                        }
                    } else {
                        // Already said when the line comes on: plainly there.
                        block.push_str("\\alpha&H00&\\fscx100\\fscy100");
                    }
                }
                if !block.is_empty() {
                    body.push_str(&format!("{{{block}}}"));
                }
                body.push_str(&ass_text(&w.text));
                if wi + 1 < line.len() {
                    body.push(' ');
                }
                seen += w.text.chars().count();
            }
        }
        out.push_str(&format!(
            "Dialogue: 0,{},{},Default,,0,0,0,,{{{tags}}}{body}\n",
            ass_time(from - win_start),
            ass_time(until - win_start),
        ));
        events += 1;
    }

    if events == 0 {
        return None;
    }
    Some(format!("{}{out}", header(look, family, scale_pct)))
}

#[cfg(test)]
#[path = "ass_tests.rs"]
mod tests;
