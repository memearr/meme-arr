// A path: where something (a layer, the caption text) is over the length of the clip,
// as keyframes `{t_ms, x, y}`. Between two keyframes it moves in a straight line; before
// the first and after the last it stays where that keyframe puts it. Times are from the
// start of the clip; places are fractions of the picture (0 left/top .. 1 right/bottom).
//
// The server does the same arithmetic when it draws the export (src/export/path.rs).

/** Most keyframes one path may have (the server's limit). */
export const MAX_PATH_POINTS = 100;
/** A keyframe set this close (ms) to an existing one replaces it instead of adding another. */
export const SAME_INSTANT_MS = 60;

const clamp = (v, lo, hi) => Math.min(hi, Math.max(lo, v));

/** The path in time order, kept inside the picture and the clip, one keyframe per instant. */
export function tidyPath(path, clipMs) {
  const kept = (path || [])
    .filter((k) => Number.isFinite(k?.t_ms) && Number.isFinite(k?.x) && Number.isFinite(k?.y))
    .map((k) => ({ t_ms: Math.round(clamp(k.t_ms, 0, Math.max(0, clipMs))), x: clamp(k.x, 0, 1), y: clamp(k.y, 0, 1) }))
    .sort((a, b) => a.t_ms - b.t_ms);
  const out = [];
  for (const k of kept) {
    if (out.length && out[out.length - 1].t_ms === k.t_ms) out[out.length - 1] = k;
    else out.push(k);
  }
  return out.slice(0, MAX_PATH_POINTS);
}

/** Where the path puts the thing `t` ms into the clip; `fallback` ({x, y}) when there is no path. */
export function positionAt(path, t, fallback) {
  if (!path || path.length === 0) return fallback;
  const first = path[0];
  const last = path[path.length - 1];
  if (t <= first.t_ms) return { x: first.x, y: first.y };
  if (t >= last.t_ms) return { x: last.x, y: last.y };
  let i = 1;
  while (path[i].t_ms <= t) i += 1;
  const a = path[i - 1];
  const b = path[i];
  const f = (t - a.t_ms) / (b.t_ms - a.t_ms);
  return { x: a.x + (b.x - a.x) * f, y: a.y + (b.y - a.y) * f };
}

/**
 * The path with a keyframe at `t` putting the thing at `(x, y)`: a keyframe within
 * `SAME_INSTANT_MS` of `t` is moved there, otherwise one is added (unless the path is full).
 */
export function setKeyframe(path, t, x, y, clipMs) {
  const at = Math.round(clamp(t, 0, Math.max(0, clipMs)));
  const next = (path || []).slice();
  const near = next.findIndex((k) => Math.abs(k.t_ms - at) <= SAME_INSTANT_MS);
  const frame = { t_ms: near >= 0 ? next[near].t_ms : at, x: clamp(x, 0, 1), y: clamp(y, 0, 1) };
  if (near >= 0) next[near] = frame;
  else if (next.length < MAX_PATH_POINTS) next.push(frame);
  return tidyPath(next, clipMs);
}

/** The path without its `index`th keyframe. */
export function removeKeyframe(path, index) {
  return (path || []).filter((_, i) => i !== index);
}

/** The path with the keyframes past the end of a clip `clipMs` long removed (it was trimmed shorter). */
export function pathForRequest(path, clipMs) {
  return tidyPath((path || []).filter((k) => k.t_ms <= clipMs), clipMs).map((k) => ({ t_ms: k.t_ms, x: k.x, y: k.y }));
}
