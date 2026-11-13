import { describe, expect, it } from "vitest";
import {
  MAX_SECTIONS,
  applyCaptionToAll,
  capLengths,
  clampSection,
  duplicate,
  layout,
  lengthOf,
  locate,
  move,
  nudgeAll,
  previewKey,
  previewParams,
  problems,
  remove,
  reverse,
  totalMs,
  trim,
} from "./sequenceModel.js";

const clip = (id, start, end, extra = {}) => ({
  id,
  rating_key: `k-${id}`,
  title: id,
  start_ms: start,
  end_ms: end,
  caption: { mode: "none" },
  subtitle_offset_ms: 0,
  ...extra,
});
const seq = () => [clip("a", 0, 2000), clip("b", 5000, 8000), clip("c", 100, 1100)];

describe("the sequence's own timeline", () => {
  it("adds up the sections and lays them end to end", () => {
    expect(totalMs(seq())).toBe(6000);
    expect(layout(seq())).toEqual([
      { id: "a", from: 0, to: 2000 },
      { id: "b", from: 2000, to: 5000 },
      { id: "c", from: 5000, to: 6000 },
    ]);
    expect(totalMs([])).toBe(0);
  });

  it("finds the section a moment falls in, and how far into it", () => {
    expect(locate(seq(), 0)).toEqual({ index: 0, offset: 0 });
    expect(locate(seq(), 1999)).toEqual({ index: 0, offset: 1999 });
    expect(locate(seq(), 2000)).toEqual({ index: 1, offset: 0 });
    expect(locate(seq(), 5500)).toEqual({ index: 2, offset: 500 });
    // past the end and before the start stay inside
    expect(locate(seq(), 99_999).index).toBe(2);
    expect(locate(seq(), -5).index).toBe(0);
    expect(locate([], 10)).toEqual({ index: -1, offset: 0 });
  });
});

describe("reordering and removing", () => {
  const ids = (s) => s.map((c) => c.id).join("");
  it("moves a section to a new place", () => {
    expect(ids(move(seq(), 0, 2))).toBe("bca");
    expect(ids(move(seq(), 2, 0))).toBe("cab");
    expect(ids(move(seq(), 1, 1))).toBe("abc");
    expect(ids(move(seq(), 0, 99))).toBe("bca"); // clamped to the end
    expect(ids(move(seq(), 9, 0))).toBe("abc"); // nothing there
  });
  it("does not change the original array", () => {
    const s = seq();
    move(s, 0, 2);
    reverse(s);
    expect(ids(s)).toBe("abc");
  });
  it("reverses, removes and duplicates in place", () => {
    expect(ids(reverse(seq()))).toBe("cba");
    expect(ids(remove(seq(), "b"))).toBe("ac");
    const d = duplicate(seq(), "b", "b2");
    expect(ids(d)).toBe("abb2c");
    expect(d[2]).toMatchObject({ start_ms: 5000, end_ms: 8000, rating_key: "k-b" });
    expect(ids(duplicate(seq(), "zzz", "x"))).toBe("abc");
  });
});

describe("trimming", () => {
  it("keeps a trim sensible", () => {
    expect(clampSection(-50, 1000)).toEqual({ start: 0, end: 1000 });
    expect(clampSection(1000, 1010)).toEqual({ start: 1000, end: 1100 });
    expect(clampSection(1000, 1010, "start")).toEqual({ start: 910, end: 1010 });
    expect(clampSection(0, 90_000)).toEqual({ start: 0, end: 60_000 });
    expect(clampSection(0, 90_000, "start")).toEqual({ start: 30_000, end: 90_000 });
  });
  it("trims one section only", () => {
    const t = trim(seq(), "b", 5500, 7000);
    expect(t[1]).toMatchObject({ start_ms: 5500, end_ms: 7000 });
    expect(t[0]).toEqual(seq()[0]);
    expect(t[2]).toEqual(seq()[2]);
  });
  it("nudges every section's start or end, never below the minimum", () => {
    const n = nudgeAll(seq(), 250, 0);
    expect(n.map((c) => [c.start_ms, c.end_ms])).toEqual([
      [250, 2000],
      [5250, 8000],
      [350, 1100],
    ]);
    const e = nudgeAll(seq(), 0, -950);
    expect(e[2].end_ms - e[2].start_ms).toBeGreaterThanOrEqual(100);
    expect(e[0]).toMatchObject({ start_ms: 0, end_ms: 1050 });
  });
  it("caps every section's length by cutting the end", () => {
    const c = capLengths(seq(), 1500);
    expect(c.map(lengthOf)).toEqual([1500, 1500, 1000]);
    expect(c[1]).toMatchObject({ start_ms: 5000, end_ms: 6500 });
  });
});

describe("one caption treatment for the whole set", () => {
  const mixed = () => [
    clip("a", 0, 1000, { caption: { mode: "none" } }),
    clip("b", 0, 1000, { caption: { mode: "meme", top: "TOP", bottom: "BOTTOM", font: "A.ttf", font_scale: 120 } }),
    clip("c", 0, 1000, { caption: { mode: "source", cues: [{ start_ms: 1, end_ms: 2, text: "x" }], font: null, font_scale: 100 }, subtitle_offset_ms: 300 }),
  ];
  it("switches every section to burned subtitles, keeping what is already burned", () => {
    const r = applyCaptionToAll(mixed(), { mode: "source" });
    expect(r[0].caption).toMatchObject({ mode: "source", cues: null });
    expect(r[1].caption).toMatchObject({ mode: "source", cues: null });
    expect(r[2].caption.cues).toHaveLength(1); // edited lines kept
    expect(r[2].subtitle_offset_ms).toBe(300);
    expect(r[0].subtitle_offset_ms).toBe(0);
  });
  it("switches every section to no captions", () => {
    const r = applyCaptionToAll(mixed(), { mode: "none" });
    expect(r.every((c) => c.caption.mode === "none" && c.subtitle_offset_ms === 0)).toBe(true);
  });
  it("sets font and size for every section that has captions, leaving meme words alone", () => {
    const r = applyCaptionToAll(mixed(), { font: "B.ttf", font_scale: 150 });
    expect(r[0].caption).toEqual({ mode: "none" });
    expect(r[1].caption).toMatchObject({ mode: "meme", top: "TOP", bottom: "BOTTOM", font: "B.ttf", font_scale: 150 });
    expect(r[2].caption).toMatchObject({ mode: "source", font: "B.ttf", font_scale: 150 });
  });
  it("an empty font means the default", () => {
    expect(applyCaptionToAll(mixed(), { font: "" })[1].caption.font).toBeNull();
  });
});

describe("limits", () => {
  it("names what would stop an export", () => {
    expect(problems(seq())).toEqual([]);
    const many = Array.from({ length: MAX_SECTIONS + 1 }, (_, i) => clip(`x${i}`, 0, 1000));
    expect(problems(many).join(" ")).toContain("limited to 20");
    const long = [clip("a", 0, 60_000), clip("b", 0, 60_000), clip("c", 0, 60_000), clip("d", 0, 1000)];
    expect(problems(long).join(" ")).toContain("3-minute");
    expect(problems([clip("a", 0, 70_000)]).join(" ")).toContain("Section 1");
  });
});

describe("previews", () => {
  it("describe a section the way the export draws it", () => {
    const meme = clip("m", 1000, 3000, { caption: { mode: "meme", top: "T", bottom: "B", font_scale: 100 } });
    expect(previewParams(meme)).toMatchObject({ captionMode: "meme", top: "T", bottom: "B", fontSize: 36, width: 480, fps: 15 });
    const cues = [{ start_ms: 1, end_ms: 2, text: "hi" }];
    const src = clip("s", 0, 1000, { caption: { mode: "source", cues, font: "A.ttf", font_scale: 130 }, subtitle_offset_ms: 250 });
    expect(previewParams(src)).toMatchObject({ captionMode: "source", offsetMs: 250, editedCues: cues, font: "A.ttf", fontScale: 130 });
    expect(previewParams(clip("n", 0, 1000))).toEqual({ start: 0, end: 1000, width: 480, fps: 15, captionMode: "none" });
  });
  it("are drawn small however large the export is", () => {
    expect(previewParams(clip("n", 0, 1000), { width: 960, fps: 30 })).toMatchObject({ width: 480, fps: 15 });
    expect(previewParams(clip("n", 0, 1000), { width: 320, fps: 10 })).toMatchObject({ width: 320, fps: 10 });
  });
  it("have a key that changes only when the picture would", () => {
    const a = clip("a", 0, 1000);
    expect(previewKey(a)).toBe(previewKey({ ...a, title: "renamed", id: "other" }));
    expect(previewKey(a)).not.toBe(previewKey({ ...a, end_ms: 1200 }));
    expect(previewKey(a)).not.toBe(previewKey({ ...a, caption: { mode: "source", cues: null } }));
  });
});
