import { describe, expect, it } from "vitest";
import {
  adjustLayer,
  canvasBox,
  isVideoAsset,
  layerActive,
  layerLabel,
  layerSrc,
  layerStyle,
  layersForRequest,
  newMediaLayer,
  newShapeLayer,
  pointToPlace,
  secondsText,
} from "./layersModel.js";

const CLIP = 5000;

describe("making layers", () => {
  it("a shape comes on where the playhead is, for a couple of seconds, in the middle", () => {
    const l = newShapeLayer("arrow", { atMs: 1200, clipMs: CLIP });
    expect(l).toMatchObject({ kind: "shape", shape: "arrow", start_ms: 1200, end_ms: 3200, x: 0.5, y: 0.5, size: 0.2, rotation: 0, opacity: 1 });
    expect(l.id).toMatch(/^layer-/);
  });

  it("is cut short by the end of the clip and never starts past it", () => {
    expect(newShapeLayer("ring", { atMs: 4500, clipMs: CLIP }).end_ms).toBe(5000);
    expect(newShapeLayer("ring", { atMs: 99_999, clipMs: CLIP }).start_ms).toBe(4900);
    expect(newShapeLayer("ring", { atMs: -50, clipMs: CLIP }).start_ms).toBe(0);
  });

  it("gives every layer its own id", () => {
    const ids = new Set([1, 2, 3].map(() => newShapeLayer("dot", { clipMs: CLIP }).id));
    expect(ids.size).toBe(3);
  });

  it("an uploaded picture is a media layer, a bit bigger", () => {
    const l = newMediaLayer({ id: "cat-1a2b3c4d.gif", name: "cat", kind: "motion" }, { atMs: 0, clipMs: CLIP });
    expect(l).toMatchObject({ kind: "media", asset: "cat-1a2b3c4d.gif", size: 0.3 });
  });
});

describe("adjusting a layer", () => {
  const base = newShapeLayer("arrow", { atMs: 1000, clipMs: CLIP });

  it("keeps place, size and opacity within what the server accepts", () => {
    const l = adjustLayer(base, { x: 4, y: -2, size: 9, opacity: 3 }, CLIP);
    expect(l).toMatchObject({ x: 1, y: 0, size: 1.5, opacity: 1 });
    expect(adjustLayer(base, { size: 0 }, CLIP).size).toBe(0.02);
  });

  it("keeps the times inside the clip, with the end after the start", () => {
    expect(adjustLayer(base, { start_ms: -300 }, CLIP).start_ms).toBe(0);
    expect(adjustLayer(base, { start_ms: 99_000 }, CLIP).start_ms).toBe(4900);
    expect(adjustLayer(base, { end_ms: 500 }, CLIP).end_ms).toBe(1100);
    expect(adjustLayer(base, { end_ms: 99_000 }, CLIP).end_ms).toBe(5000);
    // moving the start past the end pushes the end along with a minimum length
    const l = adjustLayer(base, { start_ms: 4000 }, CLIP);
    expect(l.end_ms).toBeGreaterThanOrEqual(l.start_ms + 100);
  });

  it("does not change the layer it was given", () => {
    const copy = { ...base };
    adjustLayer(base, { x: 0.1 }, CLIP);
    expect(base).toEqual(copy);
  });
});

describe("the export request", () => {
  it("sends shapes and media in the server's own shape", () => {
    const shape = { ...newShapeLayer("arrow", { atMs: 0, clipMs: CLIP }), color: "#00ff00", rotation: 45 };
    const media = newMediaLayer({ id: "a-1.png" }, { atMs: 500, clipMs: CLIP });
    const [s, m] = layersForRequest([shape, media], CLIP);
    expect(s).toEqual({ kind: "shape", shape: "arrow", color: "#00ff00", start_ms: 0, end_ms: 2000, x: 0.5, y: 0.5, size: 0.2, rotation: 45, opacity: 1 });
    expect(m).toMatchObject({ kind: "media", asset: "a-1.png", start_ms: 500 });
    expect(m).not.toHaveProperty("shape");
    expect(s).not.toHaveProperty("id");
  });

  it("sends no end for a layer that lasts to the end of the clip, and no bad colour", () => {
    const l = { ...newShapeLayer("dot", { atMs: 0, clipMs: CLIP }), end_ms: 5000, color: "red" };
    const [out] = layersForRequest([l], CLIP);
    expect(out.end_ms).toBeNull();
    expect(out).not.toHaveProperty("color");
  });

  it("drops a layer left starting at or after the end of a clip that was trimmed shorter", () => {
    const l = newShapeLayer("dot", { atMs: 4000, clipMs: CLIP });
    expect(layersForRequest([l], 3000)).toEqual([]);
    expect(layersForRequest([l], 4500)).toHaveLength(1);
    expect(layersForRequest(undefined, CLIP)).toEqual([]);
  });
});

describe("what is on screen", () => {
  const l = { start_ms: 1000, end_ms: 3000 };
  it("a layer is on from its start up to, not including, its end", () => {
    expect(layerActive(l, 999)).toBe(false);
    expect(layerActive(l, 1000)).toBe(true);
    expect(layerActive(l, 2999)).toBe(true);
    expect(layerActive(l, 3000)).toBe(false);
    expect(layerActive({ start_ms: 1000, end_ms: null }, 999_999)).toBe(true);
  });

  it("is shown from the server's own picture of it", () => {
    expect(layerSrc({ kind: "shape", shape: "arrow" })).toMatch(/api\/sprites\/arrow\.png$/);
    expect(layerSrc({ kind: "shape", shape: "ring", color: "#00ff00" })).toMatch(/ring\.png\?color=%2300ff00$/);
    expect(layerSrc({ kind: "shape", shape: "ring", color: "bad" })).not.toContain("color");
    expect(layerSrc({ kind: "media", asset: "a b-1.png" })).toMatch(/api\/overlays\/a%20b-1\.png\/file$/);
  });

  it("is laid over the picture by its centre, width and turn", () => {
    const s = layerStyle({ x: 0.25, y: 0.75, size: 0.3, rotation: 90, opacity: 0.5 });
    expect(s).toMatchObject({ left: "25%", top: "75%", width: "30%", opacity: 0.5 });
    expect(s.transform).toBe("translate(-50%, -50%) rotate(90deg)");
  });

  it("tells a video upload from a picture", () => {
    expect(isVideoAsset("a-1.mp4")).toBe(true);
    expect(isVideoAsset("a-1.WEBM")).toBe(true);
    expect(isVideoAsset("a-1.gif")).toBe(false);
    expect(isVideoAsset(undefined)).toBe(false);
  });
});

describe("dragging and naming", () => {
  it("turns a pointer position into a place in the picture, kept inside it", () => {
    const rect = { left: 100, top: 50, width: 400, height: 200 };
    expect(pointToPlace(rect, 300, 150)).toEqual({ x: 0.5, y: 0.5 });
    expect(pointToPlace(rect, 0, 9999)).toEqual({ x: 0, y: 1 });
    expect(pointToPlace(null, 1, 1)).toBeNull();
    expect(pointToPlace({ left: 0, top: 0, width: 0, height: 0 }, 1, 1)).toBeNull();
  });

  it("names a layer by its shape or its upload", () => {
    expect(layerLabel({ kind: "shape", shape: "ring" })).toBe("Ring");
    expect(layerLabel({ kind: "shape", shape: "star" })).toBe("star");
    expect(layerLabel({ kind: "media", asset: "cat-1.gif" }, [{ id: "cat-1.gif", name: "cat" }])).toBe("cat");
    expect(layerLabel({ kind: "media", asset: "gone.gif" }, [])).toBe("gone.gif");
  });

  it("shows seconds to a tenth", () => {
    expect(secondsText(1500)).toBe("1.5");
    expect(secondsText(0)).toBe("0");
    expect(secondsText(2040)).toBe("2");
  });
});

describe("the sequence canvas inside a video element", () => {
  it("is the 16:9 picture, centred, when the element is wider", () => {
    expect(canvasBox(900, 360)).toEqual({ left: 130, top: 0, width: 640, height: 360 });
  });
  it("is the whole element when it is 16:9, and fits a tall one by width", () => {
    expect(canvasBox(640, 360)).toEqual({ left: 0, top: 0, width: 640, height: 360 });
    expect(canvasBox(320, 400)).toEqual({ left: 0, top: 110, width: 320, height: 180 });
  });
  it("is nothing for an element with no size yet", () => {
    expect(canvasBox(0, 100)).toBeNull();
    expect(canvasBox(100, 0)).toBeNull();
  });
});
