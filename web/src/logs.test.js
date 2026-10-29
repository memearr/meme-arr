import { describe, expect, it } from "vitest";
import { entriesToText, formatLogLine, lastId, mergeEntries } from "./logs.js";

const e = (id, message = `m${id}`, level = "info") => ({ id, ts_ms: 0, level, target: "app", message });

describe("formatLogLine", () => {
  it("renders time, padded level, target and message", () => {
    const line = formatLogLine({ ts_ms: new Date(2026, 9, 25, 3, 4, 5, 67).getTime(), level: "warn", target: "app::index", message: "hi" });
    expect(line).toBe("03:04:05.067 WARN  app::index hi");
  });
});

describe("mergeEntries", () => {
  it("appends only entries it hasn't seen", () => {
    const merged = mergeEntries([e(1), e(2)], [e(2), e(3)]);
    expect(merged.map((x) => x.id)).toEqual([1, 2, 3]);
  });

  it("returns the same array when nothing new arrived", () => {
    const prev = [e(1)];
    expect(mergeEntries(prev, [])).toBe(prev);
  });

  it("keeps only the newest when over the cap", () => {
    const merged = mergeEntries([e(1), e(2), e(3)], [e(4), e(5)], 3);
    expect(merged.map((x) => x.id)).toEqual([3, 4, 5]);
  });
});

describe("lastId / entriesToText", () => {
  it("lastId is the newest id, or null for an empty list", () => {
    expect(lastId([e(4), e(9)])).toBe(9);
    expect(lastId([])).toBeNull();
  });

  it("downloads as one line per entry, ending in a newline", () => {
    const text = entriesToText([e(1, "a"), e(2, "b")]);
    expect(text.split("\n").filter(Boolean)).toHaveLength(2);
    expect(text.endsWith("\n")).toBe(true);
    expect(entriesToText([])).toBe("");
  });
});
