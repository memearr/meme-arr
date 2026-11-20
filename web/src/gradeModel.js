// The look of the picture: colour, mirror and fades. Pure logic, no React; the limits are the
// server's (see `Grade` in src/export/grade.rs).

export const PLAIN = {
  brightness: 0,
  contrast: 1,
  saturation: 1,
  tone: "none",
  mirror: false,
  fade_in_ms: 0,
  fade_out_ms: 0,
};

export const TONES = [
  { id: "none", label: "Colour" },
  { id: "bw", label: "Black & white" },
  { id: "sepia", label: "Sepia" },
];

/** Looks to start from: each is a whole grade (fades are left as they are). */
export const PRESETS = [
  { id: "vivid", label: "Vivid", grade: { brightness: 0.02, contrast: 1.15, saturation: 1.45, tone: "none" } },
  { id: "faded", label: "Faded", grade: { brightness: 0.06, contrast: 0.85, saturation: 0.65, tone: "none" } },
  { id: "noir", label: "Noir", grade: { brightness: -0.03, contrast: 1.35, saturation: 1, tone: "bw" } },
  { id: "sepia", label: "Old photo", grade: { brightness: 0.03, contrast: 1.05, saturation: 1, tone: "sepia" } },
];

const clamp = (v, lo, hi) => Math.min(hi, Math.max(lo, v));
const num = (v, fallback = 0) => (Number.isFinite(v) ? v : fallback);
const near = (a, b) => Math.abs(a - b) < 0.0005;

/** Whether `g` leaves the picture as it is. */
export function isPlain(g) {
  return (
    !g ||
    (near(g.brightness ?? 0, 0) &&
      near(g.contrast ?? 1, 1) &&
      near(g.saturation ?? 1, 1) &&
      (g.tone ?? "none") === "none" &&
      !g.mirror &&
      !(g.fade_in_ms > 0) &&
      !(g.fade_out_ms > 0))
  );
}

/** `grade` with `changes` applied, everything kept inside what the server takes for a `clipMs` clip. */
export function adjustGrade(grade, changes, clipMs) {
  const g = { ...PLAIN, ...grade, ...changes };
  g.brightness = Math.round(clamp(num(g.brightness), -1, 1) * 100) / 100;
  g.contrast = Math.round(clamp(num(g.contrast, 1), 0, 2) * 100) / 100;
  g.saturation = Math.round(clamp(num(g.saturation, 1), 0, 3) * 100) / 100;
  if (!TONES.some((t) => t.id === g.tone)) g.tone = "none";
  g.mirror = !!g.mirror;
  const room = Math.max(0, clipMs);
  g.fade_in_ms = Math.round(clamp(num(g.fade_in_ms), 0, room));
  // together the fades fit the clip: the one just changed keeps what it asked for
  const changedIn = changes && "fade_in_ms" in changes;
  if (g.fade_in_ms + num(g.fade_out_ms) > room) {
    if (changedIn) g.fade_out_ms = room - g.fade_in_ms;
    else g.fade_in_ms = Math.max(0, room - clamp(num(g.fade_out_ms), 0, room));
  }
  g.fade_out_ms = Math.round(clamp(num(g.fade_out_ms), 0, Math.max(0, room - g.fade_in_ms)));
  return g;
}

/** The look as the export takes it, or `undefined` for none. */
export function gradeForRequest(grade, clipMs) {
  if (isPlain(grade)) return undefined;
  const g = adjustGrade(grade, {}, clipMs);
  return {
    brightness: g.brightness,
    contrast: g.contrast,
    saturation: g.saturation,
    tone: g.tone,
    mirror: g.mirror,
    fade_in_ms: g.fade_in_ms,
    fade_out_ms: g.fade_out_ms,
  };
}

/** The player's picture with the look (close to what the export does, not exact). */
export function previewStyle(grade) {
  if (isPlain(grade)) return undefined;
  const g = { ...PLAIN, ...grade };
  const parts = [
    `brightness(${clamp(1 + g.brightness, 0, 2).toFixed(2)})`,
    `contrast(${g.contrast.toFixed(2)})`,
    `saturate(${g.saturation.toFixed(2)})`,
  ];
  if (g.tone === "bw") parts.push("grayscale(1)");
  if (g.tone === "sepia") parts.push("sepia(1)");
  const style = { filter: parts.join(" ") };
  if (g.mirror) style.transform = "scaleX(-1)";
  return style;
}

/** How far to black the picture is (0 not at all, 1 all the way) `relMs` into a `clipMs` clip. */
export function fadeLevel(grade, relMs, clipMs) {
  if (!grade) return 0;
  let level = 0;
  if (grade.fade_in_ms > 0) level = Math.max(level, 1 - relMs / grade.fade_in_ms);
  if (grade.fade_out_ms > 0) level = Math.max(level, (relMs - (clipMs - grade.fade_out_ms)) / grade.fade_out_ms);
  return clamp(level, 0, 1);
}
