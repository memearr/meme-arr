import { describe, expect, it } from "vitest";
import {
  fileSize,
  fontName,
  fontOptions,
  loadFontChoice,
  normalizeScale,
  saveFontChoice,
  uploadProblem,
  usableFont,
} from "./fonts.js";

const mem = (data = {}) => ({
  data,
  getItem: (k) => (k in data ? data[k] : null),
  setItem: (k, v) => {
    data[k] = v;
  },
});

describe("normalizeScale", () => {
  it("keeps sensible values and clamps the rest", () => {
    expect(normalizeScale(100)).toBe(100);
    expect(normalizeScale("150")).toBe(150);
    expect(normalizeScale(12)).toBe(50);
    expect(normalizeScale(9999)).toBe(300);
    expect(normalizeScale(110.4)).toBe(110);
    expect(normalizeScale("big")).toBe(100);
    expect(normalizeScale(undefined)).toBe(100);
  });
});

describe("font names and options", () => {
  it("names a font by family, adding a style only when it says something", () => {
    expect(fontName({ family: "Anton", style: "Regular" })).toBe("Anton");
    expect(fontName({ family: "Roboto", style: "Bold" })).toBe("Roboto Bold");
    expect(fontName({ family: "Impact" })).toBe("Impact");
  });

  it("lists the default first, then every font, marking the built-in ones", () => {
    const list = [
      { id: "DejaVuSans.ttf", family: "DejaVu Sans", style: "Book", builtin: true },
      { id: "Anton.ttf", family: "Anton", style: "Regular", builtin: false },
    ];
    expect(fontOptions(list, list[0])).toEqual([
      { id: "", label: "Default (DejaVu Sans Book)" },
      { id: "DejaVuSans.ttf", label: "DejaVu Sans Book (built in)" },
      { id: "Anton.ttf", label: "Anton" },
    ]);
    expect(fontOptions(undefined, null)).toEqual([{ id: "", label: "Default" }]);
  });
});

describe("uploadProblem", () => {
  const file = (name, size = 1000) => ({ name, size });
  it("accepts a plain font file", () => {
    expect(uploadProblem(file("Anton-Regular.ttf"), 16 * 1024 * 1024)).toBeNull();
    expect(uploadProblem(file("My Font (bold).OTF"), 16 * 1024 * 1024)).toBeNull();
  });
  it("explains what is wrong otherwise", () => {
    expect(uploadProblem(null)).toMatch(/choose/i);
    expect(uploadProblem(file("font.woff2"))).toMatch(/\.ttf and \.otf/);
    expect(uploadProblem(file("big.ttf", 20 * 1024 * 1024), 16 * 1024 * 1024)).toMatch(/16 MB/);
    expect(uploadProblem(file("bad;name.ttf"))).toMatch(/file name/);
  });
});

describe("fileSize", () => {
  it("is short and readable", () => {
    expect(fileSize(900)).toBe("900 B");
    expect(fileSize(131072)).toBe("128 KB");
    expect(fileSize(1.5 * 1024 * 1024)).toBe("1.5 MB");
  });
});

describe("remembering the choice", () => {
  it("round-trips, and starts at the default when nothing (or junk) is stored", () => {
    const s = mem();
    expect(loadFontChoice(s)).toEqual({ font: "", scale: 100 });
    saveFontChoice(s, "Anton.ttf", 130);
    expect(loadFontChoice(s)).toEqual({ font: "Anton.ttf", scale: 130 });
    expect(loadFontChoice(mem({ "memearr:caption-font": "{not json" }))).toEqual({ font: "", scale: 100 });
    expect(loadFontChoice(null)).toEqual({ font: "", scale: 100 });
  });

  it("drops a remembered font that is gone", () => {
    const list = [{ id: "Anton.ttf" }];
    expect(usableFont("Anton.ttf", list)).toBe("Anton.ttf");
    expect(usableFont("Deleted.ttf", list)).toBe("");
    expect(usableFont("", list)).toBe("");
  });
});
