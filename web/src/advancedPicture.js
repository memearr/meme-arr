// The advanced picture tools: how fast the clip plays and which window of the picture it keeps.
// Pure logic, no React; the limits are the server's (see src/export/reframe.rs).

/** The speeds on offer, as times normal speed. */
export const SPEEDS = [0.25, 0.5, 0.75, 1, 1.5, 2, 3, 4];

export const speedText = (s) => `${s}×`;

/** The shapes a window may take, as width over height (the whole picture has none). */
export const ASPECTS = [
  { id: "full", label: "Whole picture" },
  { id: "square", label: "Square (1:1)", ratio: 1 },
  { id: "vertical", label: "Vertical (9:16)", ratio: 9 / 16 },
  { id: "portrait", label: "Portrait (4:5)", ratio: 4 / 5 },
  { id: "wide", label: "Wide (16:9)", ratio: 16 / 9 },
];

/** The window as the editor keeps it: a shape and where its centre is (0..1 across and down). */
export const WHOLE = { aspect: "full", cx: 0.5, cy: 0.5 };

/** The smallest share of the picture a window may keep each way (the server's limit). */
const MIN_SHARE = 0.2;

const clamp = (v, lo, hi) => Math.min(hi, Math.max(lo, v));
const num = (v, fallback) => (Number.isFinite(v) ? v : fallback);

/**
 * The window `view` makes on a picture `srcAspect` wide for its height: the largest one of that shape
 * that fits, centred as asked and kept inside the picture. `null` when it would keep the whole picture.
 */
export function cropWindow(view, srcAspect) {
  const shape = ASPECTS.find((a) => a.id === view?.aspect);
  if (!shape?.ratio) return null;
  const src = srcAspect > 0 && Number.isFinite(srcAspect) ? srcAspect : 16 / 9;
  let w = 1;
  let h = 1;
  if (shape.ratio < src) w = shape.ratio / src;
  else h = src / shape.ratio;
  if (w > 0.9995 && h > 0.9995) return null;
  w = Math.max(MIN_SHARE, w);
  h = Math.max(MIN_SHARE, h);
  const cx = clamp(num(view.cx, 0.5), 0, 1);
  const cy = clamp(num(view.cy, 0.5), 0, 1);
  return { x: clamp(cx - w / 2, 0, 1 - w), y: clamp(cy - h / 2, 0, 1 - h), w, h };
}

/** A window as the export takes it (four decimals), or `null` for the whole picture. */
export function cropForRequest(win) {
  if (!win) return null;
  const r = (v) => Math.round(v * 10000) / 10000;
  const w = r(win.w);
  const h = r(win.h);
  return { x: r(Math.min(win.x, 1 - w)), y: r(Math.min(win.y, 1 - h)), w, h };
}

/** Whether a window can be moved across (it is narrower than the picture) and down (shorter). */
export function movable(win) {
  return { across: !!win && win.w < 0.9995, down: !!win && win.h < 0.9995 };
}

/** The speed as the export takes it: a speed from the list or anything inside 0.25..4, else normal. */
export function speedForRequest(speed) {
  return Number.isFinite(speed) && speed >= 0.25 && speed <= 4 ? speed : 1;
}

/** The speed a sequence section is exported at: its own, unless it has a song of its own (which can't change speed). */
export function sectionSpeed(clip) {
  return clip?.music ? 1 : speedForRequest(clip?.speed ?? 1);
}

/** The player's rate for a clip played at `clipSpeed` with the player's own speed control at `playerSpeed`. */
export function playbackRate(playerSpeed, clipSpeed) {
  return Math.min(16, Math.max(0.0625, num(playerSpeed, 1) * speedForRequest(clipSpeed)));
}

/** Where the window sits on the picture shown in `box` (`{left, top, width, height}`), for the player's guide. */
export function cropBox(win, box) {
  if (!win || !box) return null;
  return {
    left: box.left + win.x * box.width,
    top: box.top + win.y * box.height,
    width: win.w * box.width,
    height: win.h * box.height,
  };
}
