import { describe, expect, it } from "vitest";
import { captionMetrics, containedRect } from "./captionPreview.js";

describe("containedRect", () => {
  it("letterboxes a wide picture in a tall box", () => {
    expect(containedRect(800, 600, 1600, 800)).toEqual({ left: 0, top: 100, width: 800, height: 400 });
  });
  it("pillarboxes a tall picture in a wide box", () => {
    expect(containedRect(800, 400, 400, 400)).toEqual({ left: 200, top: 0, width: 400, height: 400 });
  });
  it("is null until sizes are known", () => {
    expect(containedRect(0, 0, 100, 100)).toBeNull();
    expect(containedRect(100, 100, 0, 0)).toBeNull();
  });
});

describe("captionMetrics", () => {
  it("sizes burned subtitles by the picture's height, like libass", () => {
    const small = captionMetrics({ mode: "source", width: 480, height: 270 });
    const big = captionMetrics({ mode: "source", width: 1280, height: 720 });
    expect(big.fontSize / small.fontSize).toBeCloseTo(720 / 270, 5);
    // 16 of 288 lines is 1/18 of the height, as a cell; the em is a bit smaller.
    expect(big.fontSize).toBeCloseTo((720 / 18) / 1.15, 5);
    expect(big.margin).toBeCloseTo((10 / 288) * 720, 5);
  });

  it("follows the text size slider", () => {
    const a = captionMetrics({ mode: "source", width: 640, height: 360, scalePct: 100 });
    const b = captionMetrics({ mode: "source", width: 640, height: 360, scalePct: 150 });
    expect(b.fontSize / a.fontSize).toBeCloseTo(1.5, 5);
  });

  it("sizes meme text by the width relative to the export width", () => {
    const m = captionMetrics({ mode: "meme", width: 960, height: 540, outWidth: 480, memeSize: 36 });
    expect(m.fontSize).toBeCloseTo(72, 5);
    expect(m.margin).toBeCloseTo(540 * 0.05, 5);
  });

  it("is null for an empty box", () => {
    expect(captionMetrics({ mode: "source", width: 0, height: 0 })).toBeNull();
  });
});
