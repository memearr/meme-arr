import { describe, expect, it } from "vitest";
import { effective, newCue, overlaps, retimeCue, sortCues, splitCue, textAt } from "./cueEdit.js";

const cue = { start_ms: 3000, end_ms: 4600, text: "Zero gravity, full flavor." };

describe("cue timing", () => {
  it("applies the sync offset when asked for the effective span", () => {
    expect(effective(cue, 500)).toMatchObject({ start_ms: 3500, end_ms: 5100 });
    expect(effective(cue)).toMatchObject({ start_ms: 3000, end_ms: 4600 });
  });

  it("knows which lines show inside a clip, offset included", () => {
    expect(overlaps(cue, 2000, 3500)).toBe(true);
    expect(overlaps(cue, 4600, 6000)).toBe(false);
    expect(overlaps(cue, 4600, 6000, -200)).toBe(false);
    expect(overlaps(cue, 4400, 6000, 300)).toBe(true);
  });

  it("sorts by start then end", () => {
    const a = { start_ms: 5000, end_ms: 6000, text: "b" };
    const b = { start_ms: 1000, end_ms: 2000, text: "a" };
    expect(sortCues([a, b]).map((c) => c.text)).toEqual(["a", "b"]);
  });
});

describe("retimeCue", () => {
  it("stores effective times back as indexed ones", () => {
    expect(retimeCue(cue, 3500, 5100, 500)).toMatchObject({ start_ms: 3000, end_ms: 4600 });
  });

  it("keeps a minimum length and whole milliseconds", () => {
    expect(retimeCue(cue, 3000.4, 3000, 0)).toMatchObject({ start_ms: 3000, end_ms: 3100 });
    expect(retimeCue(cue, -50, 400)).toMatchObject({ start_ms: 0 });
  });

  it("leaves the text alone", () => {
    expect(retimeCue(cue, 1000, 2000).text).toBe(cue.text);
  });
});

describe("splitCue", () => {
  it("splits the words in proportion to the cut", () => {
    const [a, b] = splitCue({ start_ms: 0, end_ms: 4000, text: "one two three four" }, 2000);
    expect(a).toMatchObject({ start_ms: 0, end_ms: 2000, text: "one two" });
    expect(b).toMatchObject({ start_ms: 2000, end_ms: 4000, text: "three four" });
  });

  it("works in effective time and stores indexed times", () => {
    const [a, b] = splitCue({ start_ms: 0, end_ms: 4000, text: "one two three four" }, 2500, 500);
    expect(a.end_ms).toBe(2000);
    expect(b.start_ms).toBe(2000);
  });

  it("always leaves a word on each side", () => {
    const [a, b] = splitCue({ start_ms: 0, end_ms: 4000, text: "one two three" }, 100);
    expect(a.text).toBe("one");
    expect(b.text).toBe("two three");
  });

  it("refuses a cut at or beyond an edge", () => {
    expect(splitCue(cue, 3000)).toBeNull();
    expect(splitCue(cue, 9000)).toBeNull();
    expect(splitCue(cue, 4580)).toBeNull();
  });
});

describe("newCue / textAt", () => {
  it("starts an empty line at the effective time", () => {
    expect(newCue(2000, 500)).toEqual({ start_ms: 1500, end_ms: 3000, text: "" });
  });

  it("finds the text showing at a moment", () => {
    const cues = [cue, { start_ms: 4000, end_ms: 5000, text: "Second" }];
    expect(textAt(cues, 3500)).toBe("Zero gravity, full flavor.");
    expect(textAt(cues, 4200)).toBe("Zero gravity, full flavor.\nSecond");
    expect(textAt(cues, 4700)).toBe("Second");
    expect(textAt(cues, 2999)).toBe("");
    expect(textAt(cues, 3600, -1000)).toBe("Second"); // shifted 1 s earlier
  });
});
