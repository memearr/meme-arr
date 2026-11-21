// Loudness as broadcast meters read it (ITU-R BS.1770 / EBU R128): the numbers behind the live LUFS
// readout. The page filters the sound with the standard's "K-weighting" (see audioGraph.js) and this
// turns the filtered samples into momentary and integrated loudness. Pure logic, no React. The browser's
// filters are the standard's shape rather than its exact coefficients, so the reading is close, not
// calibrated: the Measure button asks ffmpeg for the exact figure.

/** The quietest loudness shown, in LUFS (also the standard's absolute gate). */
export const FLOOR_LUFS = -70;
/** The length of a momentary reading, in seconds. */
export const WINDOW_S = 0.4;
/** How often a block goes into the integrated reading, in ms (75% overlap of the 400 ms window). */
export const STEP_MS = 100;
/** The most blocks kept (an hour of them). */
const MAX_BLOCKS = 36000;

/** The mean square of the last `n` samples of `buf` (all of it when `n` is not given or too big). */
export function meanSquare(buf, n = buf.length) {
  const len = Math.min(buf.length, Math.max(0, Math.floor(n)));
  if (len === 0) return 0;
  let sum = 0;
  for (let i = buf.length - len; i < buf.length; i++) sum += buf[i] * buf[i];
  return sum / len;
}

/** Loudness in LUFS of a summed channel power, or -Infinity for silence. */
export function lufsOf(power) {
  return power > 0 ? -0.691 + 10 * Math.log10(power) : -Infinity;
}

/** The power of one 400 ms block from the K-weighted samples of each channel (the channels' powers add). */
export function blockPower(channels, sampleRate) {
  const n = Math.round(WINDOW_S * sampleRate);
  return channels.reduce((sum, ch) => sum + meanSquare(ch, n), 0);
}

/**
 * Integrated loudness over a stretch: the blocks are gated (those under -70 LUFS are dropped, then those
 * more than 10 LU under the average of the rest) and what is left is averaged.
 */
export class Integrator {
  constructor() {
    this.blocks = [];
  }

  /** Take in one block's power. */
  add(power) {
    if (!(power >= 0) || !Number.isFinite(power)) return;
    this.blocks.push(power);
    if (this.blocks.length > MAX_BLOCKS) this.blocks.shift();
  }

  reset() {
    this.blocks = [];
  }

  /** The integrated loudness so far in LUFS, or -Infinity before there is anything over the gate. */
  integrated() {
    const loud = this.blocks.filter((p) => lufsOf(p) > FLOOR_LUFS);
    if (loud.length === 0) return -Infinity;
    const mean = (xs) => xs.reduce((a, b) => a + b, 0) / xs.length;
    const relative = lufsOf(mean(loud)) - 10;
    const kept = loud.filter((p) => lufsOf(p) > relative);
    return kept.length ? lufsOf(mean(kept)) : -Infinity;
  }
}

/** "-14.2 LUFS", or "-- LUFS" under the floor. */
export function lufsText(v) {
  if (!Number.isFinite(v) || v <= FLOOR_LUFS) return "-- LUFS";
  return `${(Math.round(v * 10) / 10).toFixed(1)} LUFS`;
}
