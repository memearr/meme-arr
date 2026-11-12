import { describe, expect, it } from "vitest";
import {
  clampTrim,
  fitView,
  formatTimecode,
  fpsOf,
  frameToMs,
  framesIn,
  fromPixels,
  msToFrame,
  parseTimecode,
  snapToEdges,
  snapToFrame,
  stepFrames,
  toPercent,
  zoomView,
} from "./timecode.js";

const NTSC = { num: 24000, den: 1001 }; // 23.976
const PAL = { num: 25, den: 1 };

describe("frames and time", () => {
  it("rounds a frame's time up so the picked frame is never dropped", () => {
    // frame 58 at 23.976 fps is 2419.08 ms
    expect(frameToMs(58, NTSC)).toBe(2420);
    expect(frameToMs(0, NTSC)).toBe(0);
    expect(frameToMs(25, PAL)).toBe(1000);
  });

  it("finds the nearest frame for a time", () => {
    expect(msToFrame(2419, NTSC)).toBe(58);
    expect(msToFrame(2400, NTSC)).toBe(58); // 57.5 rounds up
    expect(msToFrame(1000, PAL)).toBe(25);
  });

  it("round-trips every frame of a long film without drifting", () => {
    for (const f of [0, 1, 23, 24, 5000, 86_400, 172_800]) {
      expect(msToFrame(frameToMs(f, NTSC), NTSC)).toBe(f);
    }
  });

  it("snaps a time to a frame boundary", () => {
    expect(snapToFrame(2400, NTSC)).toBe(2420);
    expect(snapToFrame(1010, PAL)).toBe(1000);
  });

  it("steps by frames and stops at zero", () => {
    expect(stepFrames(1000, 1, PAL)).toBe(1040);
    expect(stepFrames(1000, -10, PAL)).toBe(600);
    expect(stepFrames(40, -5, PAL)).toBe(0);
  });

  it("counts frames in a span", () => {
    expect(framesIn(4700, NTSC)).toBe(113);
    expect(framesIn(2000, PAL)).toBe(50);
  });

  it("reads the API's frame rate, or null", () => {
    expect(fpsOf({ fps_num: 24000, fps_den: 1001 })).toEqual(NTSC);
    expect(fpsOf({ fps_num: 0, fps_den: 1 })).toBeNull();
    expect(fpsOf(null)).toBeNull();
  });
});

describe("parseTimecode", () => {
  it("reads seconds, with or without a fraction", () => {
    expect(parseTimecode("2.4", PAL)).toBe(2400);
    expect(parseTimecode("7", PAL)).toBe(7000);
    expect(parseTimecode("0.040", PAL)).toBe(40);
  });

  it("reads m:ss and h:mm:ss forms", () => {
    expect(parseTimecode("0:02.400", PAL)).toBe(2400);
    expect(parseTimecode("1:23.456", PAL)).toBe(83_456);
    expect(parseTimecode("00:00:02.400", PAL)).toBe(2400);
    expect(parseTimecode("1:02:03.5", PAL)).toBe(3_723_500);
    expect(parseTimecode("1:30", PAL)).toBe(90_000);
  });

  it("reads frame numbers either way round", () => {
    expect(parseTimecode("f58", NTSC)).toBe(2420);
    expect(parseTimecode("58f", NTSC)).toBe(2420);
    expect(parseTimecode(" F25 ", PAL)).toBe(1000);
  });

  it("rejects anything else", () => {
    for (const bad of ["", "  ", "abc", "1:2:3:4", "1.5:30", "-3", "f", "f1.5", null, undefined]) {
      expect(parseTimecode(bad, PAL)).toBeNull();
    }
  });
});

describe("formatTimecode", () => {
  it("formats minutes, and hours when there are any", () => {
    expect(formatTimecode(2400)).toBe("0:02.400");
    expect(formatTimecode(83_456)).toBe("1:23.456");
    expect(formatTimecode(3_723_500)).toBe("1:02:03.500");
    expect(formatTimecode(-5)).toBe("0:00.000");
  });

  it("is read back by parseTimecode", () => {
    for (const ms of [0, 1, 2400, 83_456, 3_723_500]) {
      expect(parseTimecode(formatTimecode(ms), PAL)).toBe(ms);
    }
  });
});

describe("clampTrim", () => {
  it("keeps the trim inside the media", () => {
    expect(clampTrim(-500, 3000, 10_000)).toEqual({ start: 0, end: 3000 });
    expect(clampTrim(8000, 12_000, 10_000)).toEqual({ start: 8000, end: 10_000 });
  });

  it("keeps a minimum length, giving way on the edge that was not moved", () => {
    expect(clampTrim(5000, 5000, 10_000, "end")).toEqual({ start: 5000, end: 5100 });
    expect(clampTrim(5000, 5000, 10_000, "start")).toEqual({ start: 4900, end: 5000 });
  });

  it("copes with an unknown runtime", () => {
    expect(clampTrim(1000, 4000, 0)).toEqual({ start: 1000, end: 4000 });
  });
});

describe("timeline view", () => {
  it("maps times and pixels both ways", () => {
    expect(toPercent(3000, 0, 12_000)).toBe(25);
    expect(fromPixels(250, 1000, 0, 12_000)).toBe(3000);
  });

  it("fits the clip with room either side, inside the media", () => {
    expect(fitView(4000, 8000, 60_000)).toEqual({ from: 2000, to: 10_000 });
    expect(fitView(500, 2500, 3000)).toEqual({ from: 0, to: 3000 });
  });

  it("zooms about a point and stays in bounds", () => {
    const v = zoomView({ from: 0, to: 12_000 }, 0.5, 6000, 60_000);
    expect(v).toEqual({ from: 3000, to: 9000 });
    const edge = zoomView({ from: 0, to: 12_000 }, 2, 0, 60_000);
    expect(edge.from).toBe(0);
    expect(edge.to).toBe(24_000);
    const end = zoomView({ from: 50_000, to: 60_000 }, 2, 60_000, 60_000);
    expect(end.to).toBe(60_000);
  });

  it("never zooms in past half a second or out past the filmstrip limit", () => {
    expect(zoomView({ from: 0, to: 1000 }, 0.1, 500, 60_000)).toEqual({ from: 250, to: 750 });
    const wide = zoomView({ from: 0, to: 100_000 }, 3, 0, 3_600_000);
    expect(wide.to - wide.from).toBe(120_000);
  });
});

describe("snapToEdges", () => {
  it("snaps to the nearest edge within the radius", () => {
    expect(snapToEdges(3010, [1000, 3000, 5000], 50)).toBe(3000);
    expect(snapToEdges(3100, [1000, 3000, 5000], 50)).toBe(3100);
    expect(snapToEdges(100, [], 50)).toBe(100);
  });
});
