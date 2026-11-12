// Frame <-> time arithmetic and time-field parsing for the editor's trim
// controls. Pure, so it is unit-tested without a browser.
//
// A frame rate is `{ num, den }` (24000/1001 for "23.976"): counting in
// fractions keeps frame N at the same moment however far into a film it is.
// The API takes whole milliseconds, so a frame's time is rounded UP: ffmpeg
// keeps a frame whose timestamp is at or after the start, so rounding down
// could drop the very frame the user picked.

/** The frame rate used until the file's own is known. */
export const DEFAULT_FPS = { num: 24, den: 1 };

/** `{fps_num, fps_den}` from the API -> `{num, den}`; null when unknown. */
export function fpsOf(info) {
  if (!info || !(info.fps_num > 0) || !(info.fps_den > 0)) return null;
  return { num: info.fps_num, den: info.fps_den };
}

/** Frames per second as a number. */
export function fpsValue(fps) {
  return fps.num / fps.den;
}

/** The time (whole ms, rounded up) at which frame `frame` is shown. */
export function frameToMs(frame, fps) {
  return Math.ceil((frame * 1000 * fps.den) / fps.num - 1e-6) + 0; // + 0: never -0
}

/** The frame showing at `ms` (nearest frame). */
export function msToFrame(ms, fps) {
  return Math.round((ms * fps.num) / (1000 * fps.den));
}

/** `ms` moved to the nearest frame boundary. */
export function snapToFrame(ms, fps) {
  return frameToMs(msToFrame(ms, fps), fps);
}

/** `ms` moved by `n` frames (negative = back); never before 0. */
export function stepFrames(ms, n, fps) {
  return Math.max(0, frameToMs(msToFrame(ms, fps) + n, fps));
}

/** Frames in a span of `ms`. */
export function framesIn(ms, fps) {
  return Math.round((ms * fps.num) / (1000 * fps.den));
}

/**
 * Read what someone typed into a time field. Accepts seconds (`2.4`),
 * `m:ss(.mmm)`, `h:mm:ss(.mmm)`, a frame number (`f58` or `58f`).
 * Returns whole milliseconds, or null when it can't be read.
 */
export function parseTimecode(text, fps) {
  const s = String(text ?? "")
    .trim()
    .toLowerCase();
  if (!s) return null;

  const frame = /^f\s*(\d+)$/.exec(s) || /^(\d+)\s*f$/.exec(s);
  if (frame) return frameToMs(Number(frame[1]), fps);

  const parts = s.split(":");
  if (parts.length > 3) return null;
  if (!parts.every((p) => /^\d+(\.\d+)?$/.test(p))) return null;
  // Only the last part (seconds) may carry a fraction.
  if (parts.slice(0, -1).some((p) => p.includes("."))) return null;
  const nums = parts.map(Number);
  let secs = 0;
  for (const n of nums) secs = secs * 60 + n;
  return Math.round(secs * 1000);
}

/** `m:ss.mmm` (or `h:mm:ss.mmm` from an hour up). */
export function formatTimecode(ms) {
  const total = Math.max(0, Math.round(ms));
  const h = Math.floor(total / 3_600_000);
  const m = Math.floor((total % 3_600_000) / 60_000);
  const s = Math.floor((total % 60_000) / 1000);
  const frac = String(total % 1000).padStart(3, "0");
  return h > 0
    ? `${h}:${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}.${frac}`
    : `${m}:${String(s).padStart(2, "0")}.${frac}`;
}

/** The smallest clip the editor allows. */
export const MIN_CLIP_MS = 100;
/** The longest clip the server exports. */
export const MAX_CLIP_MS = 60_000;

/**
 * A trim kept inside the media and at least `MIN_CLIP_MS` long. `moved` says
 * which edge the caller changed, so the other one gives way only when it must.
 */
export function clampTrim(start, end, duration, moved = "end") {
  const max = duration > 0 ? duration : Infinity;
  let s = Math.max(0, start);
  let e = Math.min(max, end);
  if (e - s < MIN_CLIP_MS) {
    if (moved === "start") s = Math.max(0, e - MIN_CLIP_MS);
    else e = Math.min(max, s + MIN_CLIP_MS);
    if (e - s < MIN_CLIP_MS) s = Math.max(0, e - MIN_CLIP_MS);
  }
  return { start: s, end: e };
}

/** Where `ms` falls in a view `[from, to]`, as a 0-100 percentage (unclamped). */
export function toPercent(ms, from, to) {
  return ((ms - from) / (to - from)) * 100;
}

/** The time at horizontal position `x` of a `width`-px track showing `[from, to]`. */
export function fromPixels(x, width, from, to) {
  return from + (x / width) * (to - from);
}

/**
 * The span the timeline shows: the clip with room either side (half its length,
 * at least 2 s), kept inside the media.
 */
export function fitView(start, end, duration) {
  const len = Math.max(end - start, MIN_CLIP_MS);
  const pad = Math.max(2000, len / 2);
  let from = Math.max(0, start - pad);
  let to = end + pad;
  if (duration > 0) to = Math.min(duration, to);
  if (to - from < 1000) to = from + 1000;
  return { from, to };
}

/** `view` scaled about `center` by `factor` (<1 zooms in), within `[0, duration]`. */
export function zoomView(view, factor, center, duration) {
  const span = Math.min(
    Math.max((view.to - view.from) * factor, 500),
    duration > 0 ? duration : Infinity,
    120_000 // the longest span a filmstrip can be asked for
  );
  const ratio = (center - view.from) / (view.to - view.from);
  let from = center - span * ratio;
  let to = from + span;
  if (from < 0) {
    from = 0;
    to = span;
  }
  if (duration > 0 && to > duration) {
    to = duration;
    from = Math.max(0, to - span);
  }
  return { from, to };
}

/** The nearest of `edges` to `ms` within `radius`, else `ms`. */
export function snapToEdges(ms, edges, radius) {
  let best = ms;
  let bestDist = radius + 1;
  for (const e of edges) {
    const d = Math.abs(e - ms);
    if (d < bestDist) {
      best = e;
      bestDist = d;
    }
  }
  return bestDist <= radius ? best : ms;
}
