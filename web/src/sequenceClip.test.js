import { describe, expect, it } from "vitest";
import { editorSeed, replaceClip, updatedClip } from "./sequenceClip.js";

const base = {
  id: "c1",
  rating_key: "k1",
  title: "Pilot",
  line: "Say it with pie",
  start_ms: 1000,
  end_ms: 4000,
  subtitle_offset_ms: 250,
};

describe("editorSeed", () => {
  it("restores burned subtitles with their edited lines, font, size and sync", () => {
    const cues = [{ start_ms: 1100, end_ms: 2000, text: "Say it with pie" }];
    const s = editorSeed({
      ...base,
      caption: { mode: "source", cues, font: "Impact.ttf", font_scale: 120 },
    });
    expect(s).toMatchObject({
      captionMode: "source",
      font: "Impact.ttf",
      fontScale: 120,
      subtitleOffsetMs: 250,
    });
    expect(s.editedCues).toBe(cues);
  });

  it("restores meme text", () => {
    const s = editorSeed({
      ...base,
      caption: { mode: "meme", top: "TOP", bottom: "BOTTOM", font_scale: 100 },
    });
    expect(s).toMatchObject({ captionMode: "meme", top: "TOP", bottom: "BOTTOM" });
    expect(s.editedCues).toBeNull();
  });

  it("falls back to no captions, the found line as bottom text, and no forced font", () => {
    const s = editorSeed({ ...base, caption: { mode: "none" } });
    expect(s).toMatchObject({ captionMode: "none", top: "", bottom: "Say it with pie" });
    expect(s.font).toBeUndefined();
    expect(s.fontScale).toBeUndefined();
    expect(editorSeed({ ...base }).captionMode).toBe("none");
  });

  it("restores the layers laid over the clip, or none", () => {
    const layers = [{ id: "l1", kind: "shape", shape: "dot", start_ms: 0, end_ms: null }];
    expect(editorSeed({ ...base, layers }).layers).toBe(layers);
    expect(editorSeed({ ...base }).layers).toEqual([]);
    expect(editorSeed({ ...base, layers: "nonsense" }).layers).toEqual([]);
  });

  it("treats lines of an unedited source clip as 'use the indexed ones'", () => {
    expect(editorSeed({ ...base, caption: { mode: "source", cues: null } }).editedCues).toBeNull();
  });
});

describe("updatedClip / replaceClip", () => {
  it("changes the trim, caption and sync but keeps who the clip is", () => {
    const u = updatedClip(base, {
      start: 2000,
      end: 5000,
      caption: { mode: "none" },
      subtitleOffsetMs: 0,
    });
    expect(u).toMatchObject({ id: "c1", rating_key: "k1", title: "Pilot", start_ms: 2000, end_ms: 5000 });
    expect(u.subtitle_offset_ms).toBe(0);
    expect(u.caption).toEqual({ mode: "none" });
  });

  it("keeps the layers the editor now has", () => {
    const layers = [{ id: "l1", kind: "shape", shape: "ring" }];
    const u = updatedClip({ ...base, layers: [{ id: "old" }] }, { start: 1, end: 2, caption: { mode: "none" }, subtitleOffsetMs: 0, layers });
    expect(u.layers).toBe(layers);
    expect(updatedClip(base, { start: 1, end: 2, caption: { mode: "none" }, subtitleOffsetMs: 0 }).layers).toEqual([]);
  });

  it("replaces only the clip with that id, in place", () => {
    const seq = [{ id: "a" }, { id: "b", n: 1 }, { id: "c" }];
    const next = replaceClip(seq, { id: "b", n: 2 });
    expect(next).toEqual([{ id: "a" }, { id: "b", n: 2 }, { id: "c" }]);
    expect(next[0]).toBe(seq[0]);
  });
});
