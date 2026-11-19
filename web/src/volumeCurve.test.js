import { describe, expect, it } from "vitest";
import { addPoint, curveForRequest, fitted, gainAt, gainText, linePath, movePoint, removePoint, shifted, tidy } from "./volumeCurve.js";

const p = (t_ms, gain) => ({ t_ms, gain });

describe("the volume line", () => {
  it("is steady at full volume with no points, and flat before the first and after the last", () => {
    expect(gainAt([], 1234)).toBe(1);
    expect(gainAt(undefined, 5)).toBe(1);
    const line = [p(1000, 0.5), p(3000, 0)];
    expect(gainAt(line, 0)).toBe(0.5);
    expect(gainAt(line, 4000)).toBe(0);
  });

  it("runs straight between points", () => {
    const line = [p(1000, 1), p(3000, 0)];
    expect(gainAt(line, 2000)).toBeCloseTo(0.5, 6);
    expect(gainAt(line, 1500)).toBeCloseTo(0.75, 6);
  });

  it("puts a point in time order and keeps it inside 0..1 and the clip", () => {
    const line = addPoint([p(1000, 1), p(3000, 1)], 2000, 0.3, 5000);
    expect(line.map((x) => x.t_ms)).toEqual([1000, 2000, 3000]);
    expect(addPoint([], 9000, 2, 5000)).toEqual([p(5000, 1)]);
    expect(addPoint([], -5, -1, 5000)).toEqual([p(0, 0)]);
  });

  it("moves a point that is put on top of another instead of doubling it", () => {
    const line = addPoint([p(1000, 1)], 1005, 0.2, 5000);
    expect(line).toEqual([p(1005, 0.2)]);
  });

  it("keeps a moved point between its neighbours", () => {
    const line = [p(1000, 1), p(2000, 1), p(3000, 1)];
    const moved = movePoint(line, 1, 9000, 0.4, 5000);
    expect(moved[1].t_ms).toBeLessThan(3000);
    expect(moved[1].t_ms).toBeGreaterThan(1000);
    expect(moved[1].gain).toBe(0.4);
    expect(movePoint(line, 0, -50, 0.5, 5000)[0].t_ms).toBe(0);
    expect(movePoint(line, 7, 1, 1, 5000)).toBe(line);
  });

  it("removes a point", () => {
    expect(removePoint([p(1, 1), p(100, 0.5), p(200, 0)], 1)).toEqual([p(1, 1), p(200, 0)]);
  });

  it("sends nothing for no line or a flat one, and a tidy line otherwise", () => {
    expect(curveForRequest([], 5000)).toEqual([]);
    expect(curveForRequest([p(1000, 1), p(2000, 1)], 5000)).toEqual([]);
    expect(curveForRequest([p(2000.4, 0.12345), p(1000, 1)], 5000)).toEqual([p(1000, 1), p(2000, 0.123)]);
    expect(curveForRequest([p(1000, 0)], 0)).toEqual([]);
  });

  it("tidies away points that are on top of each other and more than the server takes", () => {
    expect(tidy([p(1000, 1), p(1005, 0.5), p(2000, 0)], 5000)).toEqual([p(1000, 1), p(2000, 0)]);
    const many = Array.from({ length: 100 }, (_, i) => p(i * 40, 0.5));
    expect(tidy(many, 5000)).toHaveLength(64);
  });

  it("follows a clip whose start moves, or that gets shorter", () => {
    const line = [p(500, 1), p(2000, 0), p(4000, 1)];
    expect(shifted(line, -1000, 5000)).toEqual([p(1000, 0), p(3000, 1)]);
    expect(fitted(line, 3000)).toEqual([p(500, 1), p(2000, 0)]);
  });

  it("draws as a path with the ends carried flat to the edges", () => {
    expect(linePath([], 4000, 100, 50)).toBe("M0 0H100");
    expect(linePath([p(1000, 1), p(3000, 0)], 4000, 100, 50)).toBe("M0 0 L25 0 L75 50 L100 50");
    expect(linePath([p(1000, 1)], 0, 100, 50)).toBe("");
  });

  it("says a gain as a percentage", () => {
    expect(gainText(0.4)).toBe("40%");
  });
});
