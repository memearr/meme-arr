import { describe, expect, it } from "vitest";
import { FLOOR_DB, QUIET, audible, dbText, holdPeak, isClipping, measure, meterFill, soundForExport, toDb, toggleMute, toggleSolo } from "./mixModel.js";

describe("who is heard", () => {
  const both = { hasClip: true, hasSong: true };

  it("hears both with nothing muted or soloed", () => {
    expect(audible(QUIET, both)).toEqual({ clip: true, song: true });
    expect(audible(undefined, both)).toEqual({ clip: true, song: true });
  });

  it("silences a muted sound and leaves the other", () => {
    expect(audible(toggleMute(QUIET, "song"), both)).toEqual({ clip: true, song: false });
    expect(audible(toggleMute(QUIET, "clip"), both)).toEqual({ clip: false, song: true });
    expect(audible(toggleMute(toggleMute(QUIET, "clip"), "clip"), both)).toEqual({ clip: true, song: true });
  });

  it("lets a soloed sound silence the other, and a second click clear the solo", () => {
    const solo = toggleSolo(QUIET, "song");
    expect(audible(solo, both)).toEqual({ clip: false, song: true });
    expect(audible(toggleSolo(solo, "song"), both)).toEqual({ clip: true, song: true });
    expect(toggleSolo(solo, "clip").solo).toBe("clip");
  });

  it("keeps a muted sound silent even when it is soloed", () => {
    const m = toggleSolo(toggleMute(QUIET, "song"), "song");
    expect(audible(m, both)).toEqual({ clip: false, song: false });
  });

  it("ignores a solo on a sound that isn't there", () => {
    expect(audible({ ...QUIET, solo: "song" }, { hasClip: true, hasSong: false })).toEqual({ clip: true, song: true });
    expect(audible({ ...QUIET, solo: "clip" }, { hasClip: false, hasSong: true })).toEqual({ clip: true, song: true });
  });
});

describe("the meter numbers", () => {
  it("turns levels into dB, floored", () => {
    expect(toDb(1)).toBeCloseTo(0, 6);
    expect(toDb(0.5)).toBeCloseTo(-6.02, 1);
    expect(toDb(0)).toBe(FLOOR_DB);
    expect(toDb(1e-9)).toBe(FLOOR_DB);
  });

  it("fills the meter from the floor to full scale", () => {
    expect(meterFill(FLOOR_DB)).toBe(0);
    expect(meterFill(0)).toBe(1);
    expect(meterFill(-30)).toBeCloseTo(0.5, 6);
    expect(meterFill(6)).toBe(1);
    expect(meterFill(-90)).toBe(0);
  });

  it("measures the peak and the RMS of a block", () => {
    const m = measure(Float32Array.from([0, 0.5, -1, 0.5]));
    expect(m.peak).toBe(1);
    expect(m.rms).toBeCloseTo(Math.sqrt((0 + 0.25 + 1 + 0.25) / 4), 5);
    expect(measure(new Float32Array(0))).toEqual({ peak: 0, rms: 0 });
  });

  it("holds a peak and lets it fall 20 dB a second", () => {
    let held = holdPeak(FLOOR_DB, 1, 16);
    expect(held).toBeCloseTo(0, 6);
    held = holdPeak(held, 0, 500);
    expect(held).toBeCloseTo(-10, 6);
    expect(holdPeak(held, 0.5, 16)).toBeCloseTo(-6.02, 1); // a louder peak takes over at once
    expect(holdPeak(-59, 0, 5000)).toBe(FLOOR_DB); // never below the floor
  });

  it("says when it is clipping and writes the level", () => {
    expect(isClipping(0)).toBe(true);
    expect(isClipping(-3)).toBe(false);
    expect(dbText(-12.4)).toBe("-12 dB");
    expect(dbText(FLOOR_DB)).toBe("-∞ dB");
  });
});

describe("what the export takes", () => {
  const music = { id: "s", volume: 0.8, curve: [] };

  it("keeps the levels when everything is heard", () => {
    expect(soundForExport({ clip: true, song: true }, 1.2, music)).toEqual({ clipVolume: 1.2, music });
  });

  it("zeroes a sound that isn't heard and leaves the other alone", () => {
    expect(soundForExport({ clip: false, song: true }, 1.2, music)).toEqual({ clipVolume: 0, music });
    const got = soundForExport({ clip: true, song: false }, 1.2, music);
    expect(got.clipVolume).toBe(1.2);
    expect(got.music).toEqual({ ...music, volume: 0 });
    expect(music.volume).toBe(0.8); // the song itself isn't changed
  });

  it("has no song to silence without one", () => {
    expect(soundForExport({ clip: true, song: false }, 1, null).music).toBe(null);
  });
});
