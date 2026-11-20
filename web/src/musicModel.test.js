import { describe, expect, it } from "vitest";
import {
  adjustMusic,
  clipVolumeForRequest,
  durationText,
  musicForRequest,
  musicSrc,
  newMusic,
  coverSrc,
  percentText,
  previewState,
  songTimeText,
} from "./musicModel.js";

const CLIP = 5000;
const track = { id: "ab-1", title: "Song", artist: "Band", duration_s: 200 };

describe("choosing a song", () => {
  it("starts at the song's beginning, coming in with the clip, at full volume", () => {
    expect(newMusic(track)).toEqual({
      id: "ab-1",
      title: "Song",
      artist: "Band",
      duration_s: 200,
      cover_art: "",
      start_ms: 0,
      at_ms: 0,
      volume: 1,
      fade_in_ms: 0,
      fade_out_ms: 0,
      duck: 0,
      pan: 0,
      curve: [],
    });
  });
  it("keeps the song's cover picture id, for showing it", () => {
    expect(newMusic({ ...track, cover_art: "al-9" }).cover_art).toBe("al-9");
    expect(coverSrc("al-9", 64)).toMatch(/api\/music\/cover\/al-9\?size=64$/);
    expect(coverSrc("a b")).toMatch(/cover\/a%20b\?size=96$/);
    // and the export never sends it
    expect(musicForRequest(newMusic({ ...track, cover_art: "al-9" }), CLIP)).not.toHaveProperty("cover_art");
  });
  it("copes with a song that has no title or length", () => {
    const m = newMusic({ id: "x" });
    expect(m).toMatchObject({ title: "", artist: "", duration_s: 0 });
  });
});

describe("adjusting the music", () => {
  const base = newMusic(track);
  it("keeps the volume between silent and twice as loud", () => {
    expect(adjustMusic(base, { volume: 5 }, CLIP).volume).toBe(2);
    expect(adjustMusic(base, { volume: -1 }, CLIP).volume).toBe(0);
    expect(adjustMusic(base, { volume: NaN }, CLIP).volume).toBe(1);
  });
  it("keeps the start point inside the song", () => {
    expect(adjustMusic(base, { start_ms: -9 }, CLIP).start_ms).toBe(0);
    expect(adjustMusic(base, { start_ms: 999_999 }, CLIP).start_ms).toBe(199_000);
  });
  it("keeps the moment it comes in inside the clip, and the fades inside the time it plays", () => {
    const late = adjustMusic(base, { at_ms: 99_999 }, CLIP);
    expect(late.at_ms).toBe(4900);
    const m = adjustMusic(base, { at_ms: 3000, fade_in_ms: 9000, fade_out_ms: 1500 }, CLIP);
    expect(m.fade_in_ms).toBe(2000);
    expect(m.fade_out_ms).toBe(1500);
  });
  it("pulls the fades in when the clip is trimmed shorter", () => {
    const m = adjustMusic({ ...base, at_ms: 3000, fade_in_ms: 2000 }, {}, 3500);
    expect(m.at_ms).toBe(3000);
    expect(m.fade_in_ms).toBe(500);
  });
  it("does not change what it was given", () => {
    const copy = { ...base };
    adjustMusic(base, { volume: 0.5 }, CLIP);
    expect(base).toEqual(copy);
  });
});

describe("the export request", () => {
  it("sends the server's own fields and nothing the editor keeps for itself", () => {
    const m = { ...newMusic(track), volume: 0.5, start_ms: 30_000, at_ms: 1000, fade_in_ms: 500 };
    expect(musicForRequest(m, CLIP)).toEqual({
      id: "ab-1",
      start_ms: 30_000,
      at_ms: 1000,
      volume: 0.5,
      fade_in_ms: 500,
      fade_out_ms: 0,
      duck: 0,
      pan: 0,
      curve: [],
    });
  });
  it("sends the ducking only when the clip's own sound is kept", () => {
    const m = { ...newMusic(track), duck: 0.7 };
    expect(musicForRequest(m, CLIP, true).duck).toBe(0.7);
    expect(musicForRequest(m, CLIP, false).duck).toBe(0);
    expect(adjustMusic(m, { duck: 4 }, CLIP).duck).toBe(1);
    expect(adjustMusic(m, { duck: NaN }, CLIP).duck).toBe(0);
  });
  it("sends no music when there is none or the clip is too short for it", () => {
    expect(musicForRequest(null, CLIP)).toBeUndefined();
    expect(musicForRequest(newMusic(track), 50)).toBeUndefined();
  });
  it("only sends a clip volume when the clip's sound is kept", () => {
    expect(clipVolumeForRequest(0.5, true)).toBe(0.5);
    expect(clipVolumeForRequest(0.5, false)).toBe(1);
    expect(clipVolumeForRequest(9, true)).toBe(2);
    expect(clipVolumeForRequest(undefined, true)).toBe(1);
  });
});

describe("showing it", () => {
  it("names a song's file, volume and times", () => {
    expect(musicSrc("a b")).toMatch(/api\/music\/a%20b\/file$/);
    expect(percentText(0.75)).toBe("75%");
    expect(percentText(2)).toBe("200%");
    expect(durationText(187)).toBe("3:07");
    expect(durationText(0)).toBe("");
    expect(songTimeText(65_300)).toBe("1:05.3");
    expect(songTimeText(0)).toBe("0:00.0");
  });
});

describe("what the preview plays", () => {
  const m = { ...newMusic(track), start_ms: 30_000, at_ms: 1000, volume: 0.8, fade_in_ms: 1000, fade_out_ms: 1000 };
  it("is off until the song comes in", () => {
    expect(previewState(m, 999, CLIP).on).toBe(false);
    expect(previewState(null, 100, CLIP).on).toBe(false);
  });
  it("is at the right place in the song", () => {
    expect(previewState(m, 1000, CLIP).songMs).toBe(30_000);
    expect(previewState(m, 3500, CLIP).songMs).toBe(32_500);
  });
  it("fades in and out, and never asks the page for more than full volume", () => {
    expect(previewState(m, 1000, CLIP).gain).toBe(0);
    expect(previewState(m, 1500, CLIP).gain).toBeCloseTo(0.4, 6);
    expect(previewState(m, 3000, CLIP).gain).toBeCloseTo(0.8, 6);
    expect(previewState(m, 4500, CLIP).gain).toBeCloseTo(0.4, 6);
    expect(previewState(m, 5000, CLIP).gain).toBe(0);
    expect(previewState({ ...m, volume: 2 }, 3000, CLIP).gain).toBe(1);
  });
});
