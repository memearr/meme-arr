// How burned subtitles can look beyond the plain white-with-an-outline: a preset,
// colours, a place in the picture, word-by-word reveal and an entrance animation.
//
// The server draws these (src/export/ass.rs); this file holds the same presets and the
// same word-timing rule so the player's overlay can show the look as you edit. Keep the
// two in step: the numbers below are the ones in ass.rs.

/** `[id, label, resolved values]` -- the server's presets. Sizes are in its 288-line space. */
export const PRESETS = [
  { id: "classic", label: "Classic (white, outlined)", color: "#ffffff", outlineColor: "#000000", outline: 2, shadow: 0, bold: false, boxed: false, size: 16 },
  { id: "block", label: "Block (big yellow)", color: "#ffe600", outlineColor: "#000000", outline: 3, shadow: 0, bold: true, boxed: false, size: 36 },
  { id: "neon", label: "Neon (magenta)", color: "#ff22ff", outlineColor: "#28003c", outline: 1.5, shadow: 0, bold: true, boxed: false, size: 30 },
  { id: "horror", label: "Horror (blood red)", color: "#d81b0f", outlineColor: "#140000", outline: 2, shadow: 1, bold: true, boxed: false, size: 32 },
  { id: "soft", label: "Soft (cream, shadow)", color: "#f0ddc0", outlineColor: "#000000", outline: 0, shadow: 2, bold: true, boxed: false, size: 20 },
  { id: "box", label: "Box (on a dark bar)", color: "#ffffff", outlineColor: "#000000", outline: 3, shadow: 0, bold: false, boxed: true, size: 18 },
];

export const ANIMS = [
  { id: "none", label: "None" },
  { id: "fade", label: "Fade in" },
  { id: "pop", label: "Pop (grow)" },
  { id: "slide_up", label: "Rise into place" },
  { id: "slide_left", label: "Slide in from the right" },
];

export const ALIGNS = [
  { id: "left", label: "Left" },
  { id: "center", label: "Centre" },
  { id: "right", label: "Right" },
];

/** The words of a look are spread over this share of their line's time (ass.rs: REVEAL_SPAN). */
export const REVEAL_SPAN = 0.8;
/** How much larger a `*starred*` word is drawn (ass.rs: EMPHASIS). */
export const EMPHASIS = 1.5;

const HEX = /^#?[0-9a-fA-F]{6}$/;

/** A look that is just a preset, ready to be changed. */
export function newLook(preset = "block") {
  return { preset, align: "center", anim: "none", reveal: "none" };
}

/** The preset with this id (the first one for an unknown id). */
export function presetById(id) {
  return PRESETS.find((p) => p.id === id) || PRESETS[0];
}

/** What a look settles on: its preset, with its own colours and outline on top. */
export function resolveLook(look) {
  const base = presetById(look?.preset || "classic");
  const color = HEX.test(look?.color || "") ? look.color.replace(/^#?/, "#") : base.color;
  const outlineColor = HEX.test(look?.outline_color || "") ? look.outline_color.replace(/^#?/, "#") : base.outlineColor;
  const outline = Number.isFinite(look?.outline) ? Math.min(8, Math.max(0, look.outline)) : base.outline;
  return { ...base, color, outlineColor, outline };
}

/** What goes into a request: only the fields that are set, or null for the plain look. */
export function lookForRequest(look) {
  if (!look) return null;
  const out = { preset: look.preset || "classic", align: look.align || "center", anim: look.anim || "none", reveal: look.reveal || "none" };
  for (const k of ["color", "outline_color"]) if (HEX.test(look[k] || "")) out[k] = look[k];
  for (const k of ["outline", "x", "y"]) if (Number.isFinite(look[k])) out[k] = look[k];
  return out;
}

/**
 * The words of `text`, line by line. A `*` switches emphasis on or off (so `*two words*`
 * emphasises both) and is not shown -- the same rule as the server.
 */
export function parseWords(text) {
  const lines = [];
  let emph = false;
  for (const raw of String(text || "").split(/\r?\n/)) {
    const words = [];
    let cur = "";
    let curEmph = false;
    const flush = () => {
      if (cur) words.push({ text: cur, emphasis: curEmph });
      cur = "";
    };
    for (const ch of raw) {
      if (ch === "*") {
        emph = !emph;
        flush();
      } else if (/\s/.test(ch)) {
        flush();
      } else {
        if (!cur) curEmph = emph;
        cur += ch;
      }
    }
    flush();
    if (words.length) lines.push(words);
  }
  return lines;
}

/**
 * The words of a line with whether each is up at `nowMs`. `cue` is `{start_ms, end_ms,
 * text}` already shifted by the sync offset; without `reveal` every word is up. The
 * timing is the server's: word i is said at start + duration * 0.8 * (characters
 * before it / all characters).
 */
export function wordsAt(cue, nowMs, reveal) {
  const lines = parseWords(cue.text);
  const total = Math.max(1, lines.flat().reduce((n, w) => n + [...w.text].length, 0));
  const dur = Math.max(1, cue.end_ms - cue.start_ms);
  let seen = 0;
  return lines.map((line) =>
    line.map((w) => {
      const said = cue.start_ms + dur * REVEAL_SPAN * (seen / total);
      seen += [...w.text].length;
      return { ...w, shown: reveal !== "word" || nowMs >= Math.round(said) };
    })
  );
}

/** The first line on screen at `at` (sync offset applied), or null. */
export function cueAt(cues, at, offset = 0) {
  const c = (cues || []).find((c) => c.start_ms + offset <= at && at < c.end_ms + offset);
  return c ? { ...c, start_ms: c.start_ms + offset, end_ms: c.end_ms + offset } : null;
}

/**
 * CSS for the look's text over a picture `width` x `height` on screen: the colour, the
 * outline in the outline colour, and where it sits. `k` is the screen size of one script unit.
 */
export function lookStyle(look, k) {
  const r = resolveLook(look);
  const align = look.align || "center";
  const col = align === "left" ? 0 : align === "right" ? 2 : 1;
  const x = Number.isFinite(look.x) ? Math.min(1, Math.max(0, look.x)) : [0.05, 0.5, 0.95][col];
  const hasY = Number.isFinite(look.y);
  const o = r.outline * k;
  const shadows = [];
  if (o > 0 && !r.boxed) {
    for (const [dx, dy] of [[0, 1], [0, -1], [1, 0], [-1, 0], [1, 1], [-1, -1], [1, -1], [-1, 1]]) {
      shadows.push(`${dx * o}px ${dy * o}px 0 ${r.outlineColor}`);
    }
  }
  if (r.shadow > 0) shadows.push(`${r.shadow * k}px ${r.shadow * k}px ${r.shadow * k}px rgba(0,0,0,0.6)`);
  const style = {
    position: "absolute",
    left: `${x * 100}%`,
    textAlign: align,
    color: r.color,
    fontWeight: r.bold ? 800 : 700,
    textShadow: shadows.join(", ") || "none",
    maxWidth: "92%",
    whiteSpace: "pre-wrap",
    transform: `translate(${[0, -50, -100][col]}%, ${hasY ? -50 : 0}%)`,
  };
  if (hasY) style.top = `${Math.min(1, Math.max(0, look.y)) * 100}%`;
  else style.bottom = `${(10 / 288) * 100}%`;
  if (r.boxed) {
    style.background = "rgba(0,0,0,0.62)";
    style.padding = `${k * 2}px ${k * 4}px`;
  }
  return style;
}
