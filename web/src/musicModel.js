// Music under a clip: the song chosen from Navidrome, where it starts, how loud it and the
// clip's own sound are, and its fades. Pure logic, no React; the limits are the server's.
import { withBase } from "./base.js";

export const MAX_VOLUME = 2;
/** The shortest stretch of music worth having, so a clip trimmed short keeps a little. */
const MIN_PLAY_MS = 100;

const clamp = (v, lo, hi) => Math.min(hi, Math.max(lo, v));
const num = (v, fallback = 0) => (Number.isFinite(v) ? v : fallback);

/** A song as the music of a clip: from its start, coming in with the clip, at full volume. */
export function newMusic(track) {
  return {
    id: track.id,
    title: track.title || "",
    artist: track.artist || "",
    duration_s: num(track.duration_s),
    cover_art: track.cover_art || "",
    start_ms: 0,
    at_ms: 0,
    volume: 1,
    fade_in_ms: 0,
    fade_out_ms: 0,
    duck: 0,
  };
}

/** `music` with `changes` applied, everything kept inside what the server accepts. */
export function adjustMusic(music, changes, clipMs) {
  const m = { ...music, ...changes };
  const songMs = m.duration_s > 0 ? Math.max(0, Math.round(m.duration_s * 1000) - 1000) : 6 * 3600 * 1000;
  m.volume = clamp(num(m.volume, 1), 0, MAX_VOLUME);
  m.duck = clamp(num(m.duck), 0, 1);
  m.start_ms = Math.round(clamp(num(m.start_ms), 0, songMs));
  m.at_ms = Math.round(clamp(num(m.at_ms), 0, Math.max(0, clipMs - MIN_PLAY_MS)));
  const playing = Math.max(0, clipMs - m.at_ms);
  m.fade_in_ms = Math.round(clamp(num(m.fade_in_ms), 0, playing));
  m.fade_out_ms = Math.round(clamp(num(m.fade_out_ms), 0, playing));
  return m;
}

/** The music as the export takes it (nothing extra from the editor), or `undefined` for none.
 *  `keepAudio` is whether the clip's own sound is in the export. */
export function musicForRequest(music, clipMs, keepAudio = true) {
  if (!music || !music.id || clipMs <= MIN_PLAY_MS) return undefined;
  const m = adjustMusic(music, {}, clipMs);
  return {
    id: m.id,
    start_ms: m.start_ms,
    at_ms: m.at_ms,
    volume: m.volume,
    fade_in_ms: m.fade_in_ms,
    fade_out_ms: m.fade_out_ms,
    // ducking is for the clip's own sound, so it only goes when that is kept
    duck: keepAudio ? m.duck : 0,
  };
}

/** The clip's own volume as the export takes it: it only applies when its sound is kept. */
export function clipVolumeForRequest(volume, keepAudio) {
  return keepAudio ? clamp(num(volume, 1), 0, MAX_VOLUME) : 1;
}

/**
 * Where the stretch that plays (`playingMs` from `startMs`) sits on a `songMs` song's waveform,
 * as fractions of its width.
 */
export function songWindow(startMs, playingMs, songMs) {
  if (!(songMs > 0)) return { left: 0, width: 0 };
  const left = clamp(num(startMs) / songMs, 0, 1);
  return { left, width: clamp(num(playingMs) / songMs, 0, 1 - left) };
}

/**
 * The start point for a pick `frac` (0..1) of the way along the waveform: the playing stretch is
 * centred on the pick, kept inside the song, to the tenth of a second.
 */
export function startAtFraction(frac, songMs, playingMs) {
  const room = Math.max(0, songMs - Math.max(num(playingMs), 1000));
  const raw = clamp(num(frac), 0, 1) * songMs - num(playingMs) / 2;
  return Math.round(clamp(raw, 0, room) / 100) * 100;
}

/** The URL of a song's audio (fetched from Navidrome the first time). */
export function musicSrc(id) {
  return withBase(`/api/music/${encodeURIComponent(id)}/file`);
}

/** The URL of a song's cover picture, about `size` pixels (from Navidrome, kept by the browser). */
export function coverSrc(coverArt, size = 96) {
  return withBase(`/api/music/cover/${encodeURIComponent(coverArt)}?size=${size}`);
}

/** "75%" for a volume of 0.75. */
export function percentText(v) {
  return `${Math.round(num(v) * 100)}%`;
}

/** "3:07" for 187 seconds; "" when the length is not known. */
export function durationText(seconds) {
  if (!(seconds > 0)) return "";
  const s = Math.round(seconds);
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

/** "1:05.3" for 65 300 ms, for a position in a song. */
export function songTimeText(ms) {
  const t = Math.max(0, Math.round(num(ms) / 100) / 10);
  const m = Math.floor(t / 60);
  const s = (t - m * 60).toFixed(1).padStart(4, "0");
  return `${m}:${s}`;
}

/**
 * What the preview should be doing at `relMs` into the clip: whether the song is on yet, where
 * in the song it is, and how loud (the fades applied; the page's own volume tops out at 1).
 */
export function previewState(music, relMs, clipMs) {
  if (!music || relMs < music.at_ms) return { on: false, songMs: 0, gain: 0 };
  const into = relMs - music.at_ms;
  const playing = Math.max(0, clipMs - music.at_ms);
  let fade = 1;
  if (music.fade_in_ms > 0 && into < music.fade_in_ms) fade = Math.min(fade, into / music.fade_in_ms);
  const left = playing - into;
  if (music.fade_out_ms > 0 && left < music.fade_out_ms) fade = Math.min(fade, Math.max(0, left) / music.fade_out_ms);
  return { on: true, songMs: music.start_ms + into, gain: clamp(music.volume * fade, 0, 1) };
}
