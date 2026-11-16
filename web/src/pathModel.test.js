import { describe, expect, it } from "vitest";
import { layerPlace, layerStyle, layersForRequest, moveLayerTo, newShapeLayer } from "./layersModel.js";
import { MAX_PATH_POINTS, pathForRequest, positionAt, removeKeyframe, setKeyframe, tidyPath } from "./pathModel.js";
import { freezeLook, lookForRequest, lookPlace } from "./textLook.js";

const CLIP = 5000;
const k = (t_ms, x, y) => ({ t_ms, x, y });
const near = (a, b) => expect(a).toBeCloseTo(b, 6);

describe("a path", () => {
  it("is sorted, kept inside the picture and the clip, with one keyframe per instant", () => {
    const got = tidyPath(
      [k(2000, 0.5, 0.5), k(-5, -1, 2), k(1000, 0.2, 0.3), k(1000, 0.7, 0.8), k(500, NaN, 0.5), k(9999, 0.1, 0.1)],
      CLIP
    );
    expect(got).toEqual([k(0, 0, 1), k(1000, 0.7, 0.8), k(2000, 0.5, 0.5), k(5000, 0.1, 0.1)]);
    expect(tidyPath(undefined, CLIP)).toEqual([]);
  });

  it("is cut to the most keyframes the server takes", () => {
    const many = Array.from({ length: MAX_PATH_POINTS + 20 }, (_, i) => k(i, 0.5, 0.5));
    expect(tidyPath(many, CLIP)).toHaveLength(MAX_PATH_POINTS);
  });

  it("moves in straight lines and holds at the ends -- the same numbers as the server's test", () => {
    const p = [k(1000, 0.2, 0.4), k(3000, 0.6, 0), k(4000, 0.6, 1)];
    const at = (t) => positionAt(p, t, { x: 9, y: 9 });
    expect(at(0)).toEqual({ x: 0.2, y: 0.4 });
    expect(at(1000)).toEqual({ x: 0.2, y: 0.4 });
    near(at(2000).x, 0.4);
    near(at(2000).y, 0.2);
    expect(at(3000)).toEqual({ x: 0.6, y: 0 });
    near(at(3500).x, 0.6);
    near(at(3500).y, 0.5);
    expect(at(99_999)).toEqual({ x: 0.6, y: 1 });
  });

  it("falls back when there is no path, and one keyframe is a place to stay", () => {
    expect(positionAt([], 100, { x: 0.3, y: 0.7 })).toEqual({ x: 0.3, y: 0.7 });
    expect(positionAt(undefined, 100, { x: 0.3, y: 0.7 })).toEqual({ x: 0.3, y: 0.7 });
    expect(positionAt([k(500, 0.1, 0.2)], 9000, { x: 9, y: 9 })).toEqual({ x: 0.1, y: 0.2 });
  });
});

describe("setting and removing keyframes", () => {
  it("adds one at a new moment, in order", () => {
    let p = setKeyframe([], 1000, 0.2, 0.2, CLIP);
    p = setKeyframe(p, 3000, 0.8, 0.8, CLIP);
    p = setKeyframe(p, 2000, 0.5, 0.5, CLIP);
    expect(p.map((f) => f.t_ms)).toEqual([1000, 2000, 3000]);
  });

  it("moves the keyframe that is at about the same moment instead of crowding it", () => {
    const p = setKeyframe([k(1000, 0.2, 0.2)], 1040, 0.9, 0.9, CLIP);
    expect(p).toEqual([k(1000, 0.9, 0.9)]);
    expect(setKeyframe([k(1000, 0.2, 0.2)], 1200, 0.9, 0.9, CLIP)).toHaveLength(2);
  });

  it("keeps a keyframe inside the clip and the picture, and stops adding at the limit", () => {
    expect(setKeyframe([], 99_999, 4, -4, CLIP)).toEqual([k(5000, 1, 0)]);
    const full = Array.from({ length: MAX_PATH_POINTS }, (_, i) => k(i * 40, 0.5, 0.5));
    expect(setKeyframe(full, 4500, 0.1, 0.1, CLIP)).toHaveLength(MAX_PATH_POINTS);
  });

  it("removes by position without touching the original", () => {
    const p = [k(0, 0, 0), k(1, 1, 1), k(2, 2, 2)];
    expect(removeKeyframe(p, 1)).toEqual([k(0, 0, 0), k(2, 2, 2)]);
    expect(p).toHaveLength(3);
  });

  it("sends only keyframes inside a clip that was trimmed shorter", () => {
    const p = [k(0, 0.1, 0.1), k(2000, 0.5, 0.5), k(4000, 0.9, 0.9)];
    expect(pathForRequest(p, 3000)).toEqual([k(0, 0.1, 0.1), k(2000, 0.5, 0.5)]);
    expect(pathForRequest(undefined, 3000)).toEqual([]);
  });
});

describe("layers on a path", () => {
  const base = { ...newShapeLayer("arrow", { atMs: 0, clipMs: CLIP }), path: [k(0, 0.2, 0.5), k(2000, 0.8, 0.5)] };

  it("sit where their path has them, and in the preview move with the playhead", () => {
    near(layerPlace(base, 1000).x, 0.5);
    near(parseFloat(layerStyle(base, 1000).left), 50);
    near(parseFloat(layerStyle(base, 0).left), 20);
    // with no path, the fixed place
    expect(layerPlace({ ...base, path: [] }, 1000)).toEqual({ x: 0.5, y: 0.5 });
  });

  it("dragging adds a keyframe at the playhead when there is a path, and moves the layer when there is not", () => {
    const moved = moveLayerTo(base, { x: 0.5, y: 0.1 }, 1000, CLIP);
    expect(moved.path).toEqual([k(0, 0.2, 0.5), k(1000, 0.5, 0.1), k(2000, 0.8, 0.5)]);
    expect(moved.x).toBe(base.x);
    const still = moveLayerTo({ ...base, path: [] }, { x: 0.3, y: 0.4 }, 1000, CLIP);
    expect(still).toMatchObject({ x: 0.3, y: 0.4, path: [] });
  });

  it("send their path to the export, and nothing when there is none", () => {
    const [withPath] = layersForRequest([base], CLIP);
    expect(withPath.path).toEqual([k(0, 0.2, 0.5), k(2000, 0.8, 0.5)]);
    const [without] = layersForRequest([{ ...base, path: [] }], CLIP);
    expect(without).not.toHaveProperty("path");
  });
});

describe("text on a path", () => {
  const look = { preset: "block", path: [k(0, 0.2, 0.3), k(3000, 0.8, 0.7)] };

  it("sends its keyframes, only those inside the clip", () => {
    expect(lookForRequest(look, CLIP).path).toEqual(look.path);
    expect(lookForRequest(look, 1000).path).toEqual([k(0, 0.2, 0.3)]);
    expect(lookForRequest({ preset: "block" }, CLIP)).not.toHaveProperty("path");
  });

  it("is where its path has it, or where it was put, or nowhere in particular", () => {
    near(lookPlace(look, 1500).x, 0.5);
    expect(lookPlace({ x: 0.1, y: 0.9 }, 1500)).toEqual({ x: 0.1, y: 0.9 });
    expect(lookPlace({ preset: "block" }, 1500)).toBeNull();
    expect(lookPlace(null, 0)).toBeNull();
  });

  it("is frozen for a single frame: no path, the place it has at that moment", () => {
    const frozen = freezeLook(look, 3000);
    expect(frozen).not.toHaveProperty("path");
    expect(frozen).toMatchObject({ preset: "block", x: 0.8, y: 0.7 });
    expect(freezeLook({ preset: "block" }, 100)).toEqual({ preset: "block" });
    expect(freezeLook(null, 100)).toBeNull();
  });
});
