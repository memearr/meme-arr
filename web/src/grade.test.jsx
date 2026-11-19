// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { afterEach, describe, expect, it } from "vitest";
import { GradePanel } from "./GradePanel.jsx";
import { PLAIN, PRESETS, adjustGrade, fadeLevel, gradeForRequest, isPlain, previewStyle } from "./gradeModel.js";

afterEach(cleanup);

describe("the look model", () => {
  it("knows a plain look and sends nothing for it", () => {
    expect(isPlain(PLAIN)).toBe(true);
    expect(isPlain(undefined)).toBe(true);
    expect(gradeForRequest(PLAIN, 5000)).toBeUndefined();
    expect(previewStyle(PLAIN)).toBeUndefined();
    expect(isPlain({ ...PLAIN, mirror: true })).toBe(false);
  });

  it("keeps every value inside what the server takes", () => {
    const g = adjustGrade(PLAIN, { brightness: 5, contrast: -1, saturation: 9, tone: "neon" }, 5000);
    expect(g).toMatchObject({ brightness: 1, contrast: 0, saturation: 3, tone: "none" });
  });

  it("fits the two fades inside the clip, the one just changed winning", () => {
    let g = adjustGrade(PLAIN, { fade_out_ms: 3000 }, 4000);
    g = adjustGrade(g, { fade_in_ms: 3000 }, 4000);
    expect(g.fade_in_ms).toBe(3000);
    expect(g.fade_out_ms).toBe(1000);
    g = adjustGrade(g, { fade_out_ms: 4000 }, 4000);
    expect(g.fade_in_ms + g.fade_out_ms).toBeLessThanOrEqual(4000);
    expect(g.fade_out_ms).toBe(4000);
    expect(adjustGrade(g, {}, 1000).fade_in_ms + adjustGrade(g, {}, 1000).fade_out_ms).toBeLessThanOrEqual(1000);
  });

  it("sends the look as the export takes it", () => {
    const g = adjustGrade(PLAIN, { contrast: 1.3, tone: "bw", mirror: true, fade_in_ms: 500 }, 5000);
    expect(gradeForRequest(g, 5000)).toEqual({
      brightness: 0,
      contrast: 1.3,
      saturation: 1,
      tone: "bw",
      mirror: true,
      fade_in_ms: 500,
      fade_out_ms: 0,
    });
  });

  it("shows the look in the player as a CSS filter, and the mirror as a flip", () => {
    const s = previewStyle({ ...PLAIN, brightness: 0.1, contrast: 1.2, saturation: 0.5, tone: "sepia", mirror: true });
    expect(s.filter).toBe("brightness(1.10) contrast(1.20) saturate(0.50) sepia(1)");
    expect(s.transform).toBe("scaleX(-1)");
    expect(previewStyle({ ...PLAIN, tone: "bw" }).filter).toContain("grayscale(1)");
  });

  it("says how far to black the picture is", () => {
    const g = { ...PLAIN, fade_in_ms: 1000, fade_out_ms: 2000 };
    expect(fadeLevel(g, 0, 10000)).toBe(1);
    expect(fadeLevel(g, 500, 10000)).toBeCloseTo(0.5, 6);
    expect(fadeLevel(g, 5000, 10000)).toBe(0);
    expect(fadeLevel(g, 9000, 10000)).toBeCloseTo(0.5, 6);
    expect(fadeLevel(g, 10000, 10000)).toBe(1);
    expect(fadeLevel(PLAIN, 0, 10000)).toBe(0);
  });

  it("has starting looks the server accepts", () => {
    for (const p of PRESETS) {
      const g = adjustGrade(PLAIN, p.grade, 5000);
      expect(g.contrast).toBeGreaterThanOrEqual(0);
      expect(g.contrast).toBeLessThanOrEqual(2);
      expect(isPlain(g)).toBe(false);
    }
  });
});

function Harness({ seen }) {
  const [grade, setGrade] = useState(PLAIN);
  seen?.(grade);
  return <GradePanel grade={grade} onChange={setGrade} clipMs={5000} />;
}

describe("the Picture box", () => {
  it("applies a starting look, changes a slider and the tone, and resets", () => {
    let seen;
    render(<Harness seen={(g) => (seen = g)} />);
    fireEvent.click(screen.getByText("Noir"));
    expect(seen.tone).toBe("bw");
    expect(seen.contrast).toBeCloseTo(1.35, 2);
    fireEvent.change(screen.getByLabelText(/Saturation/), { target: { value: "0.4" } });
    expect(seen.saturation).toBeCloseTo(0.4, 2);
    fireEvent.change(screen.getByLabelText("Tone"), { target: { value: "sepia" } });
    expect(seen.tone).toBe("sepia");
    fireEvent.click(screen.getByLabelText(/Mirror/));
    expect(seen.mirror).toBe(true);
    fireEvent.click(screen.getByText("Reset"));
    expect(isPlain(seen)).toBe(true);
  });

  it("sets the fades in seconds and keeps them inside the clip", () => {
    let seen;
    render(<Harness seen={(g) => (seen = g)} />);
    fireEvent.change(screen.getByLabelText(/Fade out to black/), { target: { value: "4000" } });
    fireEvent.change(screen.getByLabelText(/Fade in from black/), { target: { value: "3000" } });
    expect(seen.fade_in_ms).toBe(3000);
    expect(seen.fade_out_ms).toBe(2000);
    expect(screen.getByText("3 s")).toBeTruthy();
  });

  it("has nothing to reset on a plain look", () => {
    render(<Harness />);
    expect(screen.getByText("Reset").disabled).toBe(true);
  });
});
