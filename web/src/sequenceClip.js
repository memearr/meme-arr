// Opening a clip of a sequence in the full editor and putting the result back.
//
// A sequence clip is what "Add to sequence" stored: the item's identity, the
// trim, the caption settings (including lines edited in the editor), the
// subtitle sync, the layers laid over it, the song under it and the look of its picture. These two functions convert between that and the editor's
// own state, so editing a clip starts exactly where it was left.

import { PLAIN, isPlain } from "./gradeModel.js";
import { WHOLE, speedForRequest } from "./advancedPicture.js";

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
    look: mode === "source" && cap.look ? cap.look : null,
    subtitleOffsetMs: clip.subtitle_offset_ms || 0,
    layers: Array.isArray(clip.layers) ? clip.layers : [],
    music: clip.music && typeof clip.music.id === "string" ? clip.music : null,
    grade: clip.grade && typeof clip.grade === "object" ? { ...PLAIN, ...clip.grade } : PLAIN,
    speed: speedForRequest(clip.speed ?? 1),
    reframe: clip.reframe && typeof clip.reframe === "object" ? { ...WHOLE, ...clip.reframe } : WHOLE,
  };
}

/** The stored clip with what the editor now says; its identity and place are kept. */
export function updatedClip(clip, { start, end, caption, subtitleOffsetMs, layers, music, grade, speed, reframe, crop }) {
  return {
    ...clip,
    start_ms: start,
    end_ms: end,
    caption,
    subtitle_offset_ms: subtitleOffsetMs,
    layers: layers || [],
    music: music || null,
    grade: grade && !isPlain(grade) ? grade : undefined,
    // how fast it plays and the window kept: `reframe` is what the editor shows, `crop` what the export takes
    speed: speed && speed !== 1 ? speed : undefined,
    reframe: reframe && reframe.aspect !== "full" ? reframe : undefined,
    crop: crop || undefined,
  };
}

/** The sequence with the clip that has `updated.id` replaced; others untouched. */
export function replaceClip(sequence, updated) {
  return sequence.map((c) => (c.id === updated.id ? updated : c));
}
