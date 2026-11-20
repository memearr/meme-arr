// Mixing the clip's own sound and the song: which of them can be heard (mute and solo), and the
// numbers behind the level meters. Pure logic, no React.

/** Nothing muted, nothing soloed. */
export const QUIET = { clipMuted: false, songMuted: false, solo: null };

/**
 * Which tracks are heard. A soloed track silences the other one; a muted track is silent even when
 * soloed. A solo on a track that isn't there (no song, or no clip sound) is ignored.
 */
export function audible(mix, { hasClip, hasSong }) {
  const m = { ...QUIET, ...mix };
  const solo = (m.solo === "clip" && hasClip) || (m.solo === "song" && hasSong) ? m.solo : null;
  return {
    clip: !m.clipMuted && (!solo || solo === "clip"),
    song: !m.songMuted && (!solo || solo === "song"),
  };
}

/** `mix` with the solo of `which` ("clip" or "song") switched: soloing the soloed track clears it. */
export function toggleSolo(mix, which) {
  return { ...QUIET, ...mix, solo: mix?.solo === which ? null : which };
}

/** `mix` with the mute of `which` switched. */
export function toggleMute(mix, which) {
  const m = { ...QUIET, ...mix };
  return which === "clip" ? { ...m, clipMuted: !m.clipMuted } : { ...m, songMuted: !m.songMuted };
}

/** The quietest level a meter shows, in dB below full scale. */
export const FLOOR_DB = -60;

/** A sample level (0..1, 1 = full scale) in dB, no lower than the meter's floor. */
export function toDb(level) {
  return level > 0 ? Math.max(FLOOR_DB, 20 * Math.log10(level)) : FLOOR_DB;
}

/** How full a meter is (0..1) for a level in dB. */
export function meterFill(db) {
  return Math.min(1, Math.max(0, (db - FLOOR_DB) / -FLOOR_DB));
}

/** The peak and the RMS of a block of samples (each -1..1). */
export function measure(samples) {
  let peak = 0;
  let sum = 0;
  for (let i = 0; i < samples.length; i++) {
    const s = samples[i];
    const a = s < 0 ? -s : s;
    if (a > peak) peak = a;
    sum += s * s;
  }
  return { peak, rms: samples.length ? Math.sqrt(sum / samples.length) : 0 };
}

/** The peak-hold marker in dB: jumps up to a louder peak, then falls 20 dB a second. */
export function holdPeak(heldDb, peak, dtMs) {
  const fall = (20 * Math.max(0, dtMs)) / 1000;
  return Math.max(toDb(peak), heldDb - fall);
}

/** Whether a level in dB is at the top of the scale (the meter's clip light). */
export function isClipping(db) {
  return db > -1;
}

/** "-12 dB" (or "-∞" at the floor) for a level in dB. */
export function dbText(db) {
  return db <= FLOOR_DB ? "-∞ dB" : `${Math.round(db)} dB`;
}

/**
 * The levels the export takes for a mix: a sound that isn't heard (muted, or not soloed) is kept at
 * zero, so the file is what the player played. `heard` is `audible(...)`'s answer.
 */
export function soundForExport(heard, clipVolume, music) {
  return {
    clipVolume: heard.clip ? clipVolume : 0,
    music: music && !heard.song ? { ...music, volume: 0 } : music,
  };
}

/** Where a pan sits, in words: "Centre", "Left 30%", "Right 100%". */
export function panText(pan) {
  const p = Number.isFinite(pan) ? Math.min(1, Math.max(-1, pan)) : 0;
  const pct = Math.round(Math.abs(p) * 100);
  if (pct === 0) return "Centre";
  return `${p < 0 ? "Left" : "Right"} ${pct}%`;
}

/** The master level the page's own sound can follow: the player tops out at full volume. */
export function previewMaster(master) {
  return Number.isFinite(master) ? Math.min(1, Math.max(0, master)) : 1;
}
