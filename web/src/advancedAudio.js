// The advanced sound tools: cuts (high-pass, low-pass), a loudness target, and a loudness reading.

/** The loudness targets on offer, in LUFS. */
export const TARGETS = [
  { lufs: -23, label: "Broadcast (-23)" },
  { lufs: -16, label: "Streaming (-16)" },
  { lufs: -14, label: "Loud (-14)" },
];
export const DEFAULT_TARGET = -16;

/** No cuts, no loudness target. */
export const PLAIN_SOUND = { clipHigh: 0, clipLow: 0, normalize: null, clipEq: { low_db: 0, mid_db: 0, high_db: 0 } };

/** The most a band of the equaliser may be turned up or down, in dB. */
export const MAX_EQ_DB = 12;
export const FLAT_EQ = { low_db: 0, mid_db: 0, high_db: 0 };

/** An equaliser kept inside the server's limits, in half-dB steps (anything missing is flat). */
export function adjustEq(eq, changes = {}) {
  const e = { ...FLAT_EQ, ...eq, ...changes };
  const band = (v) => {
    const n = Number(v);
    return Number.isFinite(n) ? Math.round(Math.min(MAX_EQ_DB, Math.max(-MAX_EQ_DB, n)) * 2) / 2 : 0;
  };
  return { low_db: band(e.low_db), mid_db: band(e.mid_db), high_db: band(e.high_db) };
}

/** Whether an equaliser leaves the sound as it is. */
export function isFlatEq(eq) {
  const e = adjustEq(eq);
  return e.low_db === 0 && e.mid_db === 0 && e.high_db === 0;
}

/** "+3 dB", "-1.5 dB", "0 dB". */
export function dbBandText(db) {
  const v = Number.isFinite(db) ? db : 0;
  return `${v > 0 ? "+" : ""}${v} dB`;
}

export const HIGH_RANGE = [20, 2000];
export const LOW_RANGE = [1000, 20000];
const STEPS = 100;

/** A slider position (0 off, 1..100 spread evenly by pitch) as Hz within `range`; 0 is off. */
export function stepToHz(step, [lo, hi]) {
  const s = Math.round(Number(step));
  if (!Number.isFinite(s) || s <= 0) return 0;
  return Math.round(lo * (hi / lo) ** (Math.min(s, STEPS) / STEPS));
}

/** The slider position of `hz` within `range` (the inverse of `stepToHz`). */
export function hzToStep(hz, [lo, hi]) {
  if (!Number.isFinite(hz) || hz <= 0) return 0;
  const s = Math.round((Math.log(Math.min(hi, Math.max(lo, hz)) / lo) / Math.log(hi / lo)) * STEPS);
  return Math.min(STEPS, Math.max(1, s));
}

/** "Off", "120 Hz", "4.5 kHz". */
export function hzText(hz) {
  if (!hz) return "Off";
  return hz >= 1000 ? `${Math.round(hz / 100) / 10} kHz` : `${Math.round(hz)} Hz`;
}

/** What a loudness reading says: "-14.9 LUFS, peak -0.4 dB, range 5.3 LU", or why there is none. */
export function loudnessText(r) {
  if (!r) return "";
  if (r.has_audio === false) return "This clip has no sound.";
  if (r.lufs == null) return "That stretch is silent.";
  const n = (v) => (Math.round(v * 10) / 10).toFixed(1);
  return `${n(r.lufs)} LUFS, peak ${n(r.true_peak_db)} dB, range ${n(r.range_lu)} LU`;
}

/** A cut from the song or a request: off (0) or kept within `lo`..`hi`. */
export function cutHz(v, lo, hi) {
  const n = Number(v);
  if (!Number.isFinite(n) || n <= 0) return 0;
  return Math.round(Math.min(hi, Math.max(lo, n)));
}

/** The advanced sound settings for the export's `mixdown`: the plain ones unless Advanced is on and the sound is kept. */
export function soundForMixdown(sound, { advanced, audio }) {
  if (!advanced || !audio) return { clip_highpass_hz: 0, clip_lowpass_hz: 0, normalize_lufs: null, clip_eq: FLAT_EQ };
  return {
    clip_highpass_hz: sound.clipHigh || 0,
    clip_lowpass_hz: sound.clipLow || 0,
    normalize_lufs: sound.normalize ?? null,
    clip_eq: adjustEq(sound.clipEq),
  };
}

/** The song as a request should have it: its cuts and equaliser only count with Advanced on. */
export function songCuts(music, advanced) {
  return music && !advanced ? { ...music, highpass_hz: 0, lowpass_hz: 0, eq: FLAT_EQ } : music;
}
