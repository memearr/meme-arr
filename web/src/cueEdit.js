// Editing the subtitle lines that get burned in: retime, split, add. Pure.
//
// A line's stored times are as indexed; the sync offset is applied when it is
// drawn or burned. The editor shows and edits the *effective* time (stored +
// offset), so these helpers take the offset where it matters.

/** The effective (offset-applied) span of a line. */
export function effective(cue, offset = 0) {
  return { ...cue, start_ms: cue.start_ms + offset, end_ms: cue.end_ms + offset };
}

/** Whether a line shows during any part of `[start, end)` once offset is applied. */
export function overlaps(cue, start, end, offset = 0) {
  return cue.end_ms + offset > start && cue.start_ms + offset < end;
}

/** Lines in time order. */
export function sortCues(cues) {
  return [...cues].sort((a, b) => a.start_ms - b.start_ms || a.end_ms - b.end_ms);
}

/**
 * A line moved or resized to the effective times `[start, end)`. The end never
 * comes before the start (at least 100 ms long), and times are whole ms.
 */
export function retimeCue(cue, start, end, offset = 0) {
  const s = Math.max(0, Math.round(start));
  const e = Math.max(s + 100, Math.round(end));
  return { ...cue, start_ms: s - offset, end_ms: e - offset };
}

/**
 * Split a line in two at the effective time `at`, dividing the words in
 * proportion to where the cut falls. Returns null when `at` isn't inside the
 * line or the line is too short to cut.
 */
export function splitCue(cue, at, offset = 0) {
  const s = cue.start_ms + offset;
  const e = cue.end_ms + offset;
  if (at <= s + 50 || at >= e - 50) return null;
  const words = cue.text.split(/\s+/).filter(Boolean);
  const cut = words.length < 2 ? words.length : Math.min(words.length - 1, Math.max(1, Math.round((words.length * (at - s)) / (e - s))));
  const cutAt = at - offset;
  return [
    { ...cue, end_ms: cutAt, text: words.slice(0, cut).join(" ") },
    { ...cue, start_ms: cutAt, text: words.slice(cut).join(" ") },
  ];
}

/** A new empty line starting at the effective time `at`. */
export function newCue(at, offset = 0, length = 1500) {
  return { start_ms: Math.max(0, Math.round(at)) - offset, end_ms: Math.max(0, Math.round(at)) - offset + length, text: "" };
}

/** The text showing at the effective time `at`, lines joined by a newline. */
export function textAt(cues, at, offset = 0) {
  return cues
    .filter((c) => c.start_ms + offset <= at && at < c.end_ms + offset)
    .map((c) => c.text)
    .filter(Boolean)
    .join("\n");
}
