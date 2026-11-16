import { describe, expect, it } from "vitest";
import { cueAt, lookForRequest, lookStyle, newLook, parseWords, presetById, resolveLook, wordsAt } from "./textLook.js";

describe("presets and looks", () => {
  it("knows its presets and falls back to the first for an unknown one", () => {
    expect(presetById("block").color).toBe("#ffe600");
    expect(presetById("nope").id).toBe("classic");
  });

  it("settles on the preset, with the look's own colours on top", () => {
    expect(resolveLook({ preset: "block" }).size).toBe(36);
    expect(resolveLook({ preset: "block", color: "#00ff00" }).color).toBe("#00ff00");
    // a bad colour or an out-of-range outline is ignored or kept in range
    expect(resolveLook({ preset: "block", color: "red" }).color).toBe("#ffe600");
    expect(resolveLook({ preset: "block", outline: 99 }).outline).toBe(8);
    expect(resolveLook(null).id).toBe("classic");
  });

  it("sends only what is set, and nothing for the plain look", () => {
    expect(lookForRequest(null)).toBeNull();
    expect(lookForRequest(newLook("neon"))).toEqual({ preset: "neon", align: "center", anim: "none", reveal: "none" });
    const full = lookForRequest({ ...newLook("box"), color: "#112233", x: 0.2, y: 0.9, anim: "pop", reveal: "word", junk: 1 });
    expect(full).toEqual({ preset: "box", align: "center", anim: "pop", reveal: "word", color: "#112233", x: 0.2, y: 0.9 });
    expect(lookForRequest({ preset: "box", color: "nope", x: "left" })).not.toHaveProperty("color");
    expect(lookForRequest({ preset: "box", x: "left" })).not.toHaveProperty("x");
  });
});

describe("words", () => {
  it("splits lines into words and treats stars as emphasis switches", () => {
    expect(parseWords("Well, this *oddity* indeed")).toEqual([
      [
        { text: "Well,", emphasis: false },
        { text: "this", emphasis: false },
        { text: "oddity", emphasis: true },
        { text: "indeed", emphasis: false },
      ],
    ]);
    const two = parseWords("*two words* then\nnext")[0];
    expect(two.map((w) => w.emphasis)).toEqual([true, true, false]);
    expect(parseWords("one\ntwo")).toHaveLength(2);
    expect(parseWords(" * ")).toEqual([]);
  });

  it("times the words the way the server does: 80% of the line, by characters", () => {
    // the server's own numbers for "aaaa bbbb cccc dddd" over 4 s: 0, 800, 1600 and 2400 ms
    const cue = { start_ms: 0, end_ms: 4000, text: "aaaa bbbb cccc dddd" };
    const shownAt = (t) => wordsAt(cue, t, "word")[0].map((w) => w.shown);
    expect(shownAt(0)).toEqual([true, false, false, false]);
    expect(shownAt(799)).toEqual([true, false, false, false]);
    expect(shownAt(800)).toEqual([true, true, false, false]);
    expect(shownAt(2400)).toEqual([true, true, true, true]);
    // without the reveal everything is up from the start
    expect(wordsAt(cue, 0, "none")[0].every((w) => w.shown)).toBe(true);
  });

  it("starts from the cue's own time, not the clip's", () => {
    const cue = { start_ms: 10_000, end_ms: 14_000, text: "aaaa bbbb" };
    expect(wordsAt(cue, 10_000, "word")[0].map((w) => w.shown)).toEqual([true, false]);
    // 10000 + 4000 * 0.8 * (4 / 8) = 11600
    expect(wordsAt(cue, 11_600, "word")[0].map((w) => w.shown)).toEqual([true, true]);
  });
});

describe("cueAt", () => {
  const cues = [
    { start_ms: 1000, end_ms: 2000, text: "a" },
    { start_ms: 3000, end_ms: 4000, text: "b" },
  ];
  it("finds the line on screen, shifted by the sync offset", () => {
    expect(cueAt(cues, 1500)).toEqual({ start_ms: 1000, end_ms: 2000, text: "a" });
    expect(cueAt(cues, 2500)).toBeNull();
    // with +500 ms, b runs 3500..4500
    expect(cueAt(cues, 3499, 500)).toBeNull();
    expect(cueAt(cues, 3500, 500)).toEqual({ start_ms: 3500, end_ms: 4500, text: "b" });
    expect(cueAt(cues, 4500, 500)).toBeNull();
    expect(cueAt(null, 0)).toBeNull();
  });
});

describe("lookStyle", () => {
  it("rests on the bottom, centred, until it is placed", () => {
    const s = lookStyle(newLook("block"), 2);
    expect(s.left).toBe("50%");
    expect(s.bottom).toMatch(/%$/);
    expect(s).not.toHaveProperty("top");
    expect(s.color).toBe("#ffe600");
    expect(s.transform).toBe("translate(-50%, 0%)");
  });

  it("centres on the chosen spot, by alignment", () => {
    const s = lookStyle({ ...newLook("block"), x: 0.25, y: 0.5, align: "left" }, 2);
    expect(s.left).toBe("25%");
    expect(s.top).toBe("50%");
    expect(s.transform).toBe("translate(0%, -50%)");
    expect(lookStyle({ ...newLook("block"), align: "right" }, 2).left).toBe("95%");
  });

  it("outlines in the outline colour, or draws a bar for the box preset", () => {
    expect(lookStyle(newLook("block"), 2).textShadow).toContain("0 #000000");
    expect(lookStyle({ ...newLook("block"), outline_color: "#ff0000" }, 2).textShadow).toContain("0 #ff0000");
    const box = lookStyle(newLook("box"), 2);
    expect(box.background).toContain("rgba(0,0,0");
    expect(box.textShadow).toBe("none");
  });
});
