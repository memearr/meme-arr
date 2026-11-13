// The sequence as one thing: its sections laid end to end, and the edits that
// apply to the set (reorder, duplicate, trim one or all, captions for all).
// Pure functions over the array of stored clips ("sections"); the screen only
// draws what these return.

import { MAX_CLIP_MS, MIN_CLIP_MS } from "./timecode.js";

/** The server's limits for one sequence export. */
export const MAX_SECTIONS = 20;
export const MAX_TOTAL_MS = 180_000;
export { MAX_CLIP_MS as MAX_SECTION_MS, MIN_CLIP_MS as MIN_SECTION_MS };

export const lengthOf = (c) => Math.max(0, c.end_ms - c.start_ms);

export function totalMs(seq) {
  return seq.reduce((sum, c) => sum + lengthOf(c), 0);
}

/** Where each section sits on the sequence's own timeline: `[{ id, from, to }]`. */
export function layout(seq) {
  let at = 0;
  return seq.map((c) => {
    const from = at;
    at += lengthOf(c);
    return { id: c.id, from, to: at };
  });
}

/** Which section a moment of the whole sequence falls in, and how far into it. */
export function locate(seq, t) {
  if (seq.length === 0) return { index: -1, offset: 0 };
  const spans = layout(seq);
  const total = spans[spans.length - 1].to;
  const at = Math.min(Math.max(0, t), Math.max(0, total - 1));
  const index = spans.findIndex((s) => at < s.to);
  const i = index < 0 ? seq.length - 1 : index;
  return { index: i, offset: at - spans[i].from };
}

/** Move the section at `from` so it sits at `to` (both clamped); a new array. */
export function move(seq, from, to) {
  if (from < 0 || from >= seq.length) return seq;
  const dest = Math.min(Math.max(0, to), seq.length - 1);
  if (dest === from) return seq;
  const copy = [...seq];
  const [item] = copy.splice(from, 1);
  copy.splice(dest, 0, item);
  return copy;
}

export function reverse(seq) {
  return [...seq].reverse();
}

export function remove(seq, id) {
  return seq.filter((c) => c.id !== id);
}

/** A copy of section `id`, right after it, with the id `newId`. */
export function duplicate(seq, id, newId) {
  const i = seq.findIndex((c) => c.id === id);
  if (i < 0) return seq;
  const copy = [...seq];
  copy.splice(i + 1, 0, { ...seq[i], id: newId });
  return copy;
}

/** A trim that stays sensible: not before 0, at least the minimum, at most the section limit. */
export function clampSection(start, end, moved = "end") {
  let s = Math.max(0, Math.round(start));
  let e = Math.round(end);
  if (e - s < MIN_CLIP_MS) {
    if (moved === "start") s = e - MIN_CLIP_MS;
    else e = s + MIN_CLIP_MS;
    if (s < 0) {
      s = 0;
      e = MIN_CLIP_MS;
    }
  }
  if (e - s > MAX_CLIP_MS) {
    if (moved === "start") s = e - MAX_CLIP_MS;
    else e = s + MAX_CLIP_MS;
  }
  return { start: s, end: e };
}

/** Section `id` with a new trim. */
export function trim(seq, id, start, end, moved = "end") {
  const t = clampSection(start, end, moved);
  return seq.map((c) => (c.id === id ? { ...c, start_ms: t.start, end_ms: t.end } : c));
}

/** Every section's start moved by `startDelta` and end by `endDelta` (ms, either sign). */
export function nudgeAll(seq, startDelta, endDelta) {
  return seq.map((c) => {
    const moved = startDelta !== 0 && endDelta === 0 ? "start" : "end";
    const t = clampSection(c.start_ms + startDelta, c.end_ms + endDelta, moved);
    return { ...c, start_ms: t.start, end_ms: t.end };
  });
}

/** No section longer than `maxMs`: longer ones lose the end. */
export function capLengths(seq, maxMs) {
  const cap = Math.max(MIN_CLIP_MS, Math.round(maxMs));
  return seq.map((c) => (lengthOf(c) > cap ? { ...c, end_ms: c.start_ms + cap } : c));
}

/**
 * The same caption treatment for every section. `patch` may carry `mode`
 * ("none" | "source"), `font` and `font_scale`. Meme text differs per section, so
 * a section that already has it keeps its words and only takes the font and size.
 */
export function applyCaptionToAll(seq, patch) {
  return seq.map((c) => {
    const cur = c.caption || { mode: "none" };
    let next = cur;
    if (patch.mode === "none") next = { mode: "none" };
    else if (patch.mode === "source" && cur.mode !== "source")
      next = { mode: "source", cues: null, font: cur.font ?? null, font_scale: cur.font_scale ?? 100 };
    if (next.mode !== "none") {
      if ("font" in patch) next = { ...next, font: patch.font || null };
      if ("font_scale" in patch) next = { ...next, font_scale: patch.font_scale };
    }
    return { ...c, caption: next, subtitle_offset_ms: next.mode === "source" ? c.subtitle_offset_ms || 0 : 0 };
  });
}

/** Plain-words problems that stop an export (the server enforces the same). */
export function problems(seq) {
  const out = [];
  if (seq.length > MAX_SECTIONS) out.push(`Sequences are limited to ${MAX_SECTIONS} sections.`);
  if (totalMs(seq) > MAX_TOTAL_MS) out.push("The sequence is over the 3-minute limit.");
  const long = seq.findIndex((c) => lengthOf(c) > MAX_CLIP_MS);
  if (long >= 0) out.push(`Section ${long + 1} is longer than 60 seconds.`);
  return out;
}

/** The query for one section's preview, the way the export will draw it. */
export function previewParams(c, { width = 480, fps = 15 } = {}) {
  const cap = c.caption || { mode: "none" };
  const base = {
    start: c.start_ms,
    end: c.end_ms,
    width: Math.min(width, 480),
    fps: Math.min(fps, 15),
    captionMode: cap.mode === "none" ? "none" : cap.mode,
  };
  if (cap.mode === "source") {
    return {
      ...base,
      offsetMs: c.subtitle_offset_ms || 0,
      editedCues: Array.isArray(cap.cues) ? cap.cues : undefined,
      font: cap.font || undefined,
      fontScale: cap.font_scale || undefined,
    };
  }
  if (cap.mode === "meme") {
    return {
      ...base,
      top: cap.top || undefined,
      bottom: cap.bottom || undefined,
      fontSize: cap.font_size ?? 36,
      font: cap.font || undefined,
      fontScale: cap.font_scale || undefined,
    };
  }
  return base;
}

/** A key that changes exactly when a section's preview would look different. */
export function previewKey(c, opts) {
  return JSON.stringify([c.rating_key, previewParams(c, opts)]);
}
