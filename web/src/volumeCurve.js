// A volume line: points of {t_ms, gain} over a clip, the volume moving straight between them.
// gain is 0 (silent) to 1 (the volume as set); before the first point it is the first one's,
// after the last it is the last's; no points means steady. Pure logic, no React; the limits are
// the server's (see `VolumePoint` in src/export/music.rs).

export const MAX_POINTS = 64;
/** Points closer than this in time are the same point (and the server wants them apart). */
export const MIN_GAP_MS = 20;

const clamp = (v, lo, hi) => Math.min(hi, Math.max(lo, v));
const num = (v, fallback = 0) => (Number.isFinite(v) ? v : fallback);

/** How loud (0..1) the line says it is at `t` ms. */
export function gainAt(points, t) {
  if (!points || points.length === 0) return 1;
  if (t <= points[0].t_ms) return points[0].gain;
  const last = points[points.length - 1];
  if (t >= last.t_ms) return last.gain;
  for (let i = 1; i < points.length; i++) {
    const b = points[i];
    if (t < b.t_ms) {
      const a = points[i - 1];
      return a.gain + ((b.gain - a.gain) * (t - a.t_ms)) / (b.t_ms - a.t_ms);
    }
  }
  return last.gain;
}

/** `points` cleaned up for a `clipMs` clip: rounded, in range, in order, one per moment. */
export function tidy(points, clipMs) {
  const out = [];
  const sorted = (points || [])
    .map((p) => ({ t_ms: Math.round(clamp(num(p.t_ms), 0, Math.max(0, clipMs))), gain: Math.round(clamp(num(p.gain, 1), 0, 1) * 1000) / 1000 }))
    .sort((a, b) => a.t_ms - b.t_ms);
  for (const p of sorted) {
    if (out.length && p.t_ms - out[out.length - 1].t_ms < MIN_GAP_MS) continue;
    out.push(p);
  }
  return out.slice(0, MAX_POINTS);
}

/** A point put on the line at `t` ms with `gain`. Too near another point: that one is moved. */
export function addPoint(points, t, gain, clipMs) {
  const list = points || [];
  const near = list.findIndex((p) => Math.abs(p.t_ms - t) < MIN_GAP_MS);
  if (near >= 0) return movePoint(list, near, t, gain, clipMs);
  if (list.length >= MAX_POINTS) return list;
  return tidy([...list, { t_ms: t, gain }], clipMs);
}

/** Point `i` moved to `t` ms and `gain`, kept between its neighbours and the ends of the clip. */
export function movePoint(points, i, t, gain, clipMs) {
  const list = points || [];
  if (i < 0 || i >= list.length) return list;
  const lo = i > 0 ? list[i - 1].t_ms + MIN_GAP_MS : 0;
  const hi = i < list.length - 1 ? list[i + 1].t_ms - MIN_GAP_MS : clipMs;
  const next = list.slice();
  next[i] = {
    t_ms: Math.round(clamp(num(t), lo, Math.max(lo, hi))),
    gain: Math.round(clamp(num(gain, 1), 0, 1) * 1000) / 1000,
  };
  return next;
}

/** The line without point `i`. */
export function removePoint(points, i) {
  return (points || []).filter((_, k) => k !== i);
}

/** The line as the export takes it, or `[]` (no line) when it is empty, flat at full, or the clip is too short. */
export function curveForRequest(points, clipMs) {
  if (!points || points.length === 0 || !(clipMs > 0)) return [];
  const clean = tidy(points, clipMs);
  if (clean.every((p) => p.gain === 1)) return [];
  return clean;
}

/** The line moved by `deltaMs` (the clip's start moving), points that fall off the clip dropped. */
export function shifted(points, deltaMs, clipMs) {
  return tidy(
    (points || []).map((p) => ({ ...p, t_ms: p.t_ms + deltaMs })).filter((p) => p.t_ms >= 0 && p.t_ms <= clipMs),
    clipMs
  );
}

/** The line cut to a clip that is now `clipMs` long (points past the end are dropped). */
export function fitted(points, clipMs) {
  return tidy((points || []).filter((p) => p.t_ms <= clipMs), clipMs);
}

/**
 * The line as an SVG path over a `w` x `h` box: the whole clip across, gain 1 at the top. It
 * runs flat from the first point back to the start, and from the last to the end.
 */
export function linePath(points, clipMs, w, h) {
  if (!(clipMs > 0)) return "";
  const x = (t) => (clamp(t, 0, clipMs) / clipMs) * w;
  const y = (g) => (1 - clamp(g, 0, 1)) * h;
  if (!points || points.length === 0) return `M0 ${y(1)}H${w}`;
  const first = points[0];
  const last = points[points.length - 1];
  const parts = [`M0 ${y(first.gain)}`];
  for (const p of points) parts.push(`L${x(p.t_ms)} ${y(p.gain)}`);
  parts.push(`L${w} ${y(last.gain)}`);
  return parts.join(" ");
}

/** "40%" for gain 0.4. */
export function gainText(g) {
  return `${Math.round(num(g, 1) * 100)}%`;
}
