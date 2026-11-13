// Opening a clip of a sequence in the full editor and putting the result back.
//
// A sequence clip is what "Add to sequence" stored: the item's identity, the
// trim, the caption settings (including lines edited in the editor) and the
// subtitle sync. These two functions convert between that and the editor's
// own state, so editing a clip starts exactly where it was left.

/** The editor's starting state for a stored clip. */
export function editorSeed(clip) {
  const cap = clip.caption || { mode: "none" };
  const mode = cap.mode === "source" || cap.mode === "meme" ? cap.mode : "none";
  return {
    captionMode: mode,
    top: mode === "meme" ? cap.top || "" : "",
    // Meme text starts from its own bottom line; otherwise from the line the clip was found by.
    bottom: mode === "meme" ? cap.bottom || "" : clip.line || "",
    font: cap.font || undefined,
    fontScale: cap.font_scale || undefined,
    editedCues: mode === "source" && Array.isArray(cap.cues) ? cap.cues : null,
    subtitleOffsetMs: clip.subtitle_offset_ms || 0,
  };
}

/** The stored clip with what the editor now says; its identity and place are kept. */
export function updatedClip(clip, { start, end, caption, subtitleOffsetMs }) {
  return {
    ...clip,
    start_ms: start,
    end_ms: end,
    caption,
    subtitle_offset_ms: subtitleOffsetMs,
  };
}

/** The sequence with the clip that has `updated.id` replaced; others untouched. */
export function replaceClip(sequence, updated) {
  return sequence.map((c) => (c.id === updated.id ? updated : c));
}
