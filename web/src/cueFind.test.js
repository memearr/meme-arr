import { describe, expect, it } from "vitest";
import { findCueMatches, stepMatch } from "./cueFind.js";

const cue = (text) => ({ start_ms: 0, end_ms: 0, text });

describe("findCueMatches", () => {
  const cues = [cue("I am the one who knocks"), cue("Say my name"), cue("Who knocks?")];

  it("returns the indices of every matching line, in order", () => {
    expect(findCueMatches(cues, "knocks")).toEqual([0, 2]);
  });

  it("is case-insensitive and ignores surrounding whitespace", () => {
    expect(findCueMatches(cues, "  WHO  ")).toEqual([0, 2]);
  });

  it("returns nothing for an empty or whitespace-only query", () => {
    expect(findCueMatches(cues, "")).toEqual([]);
    expect(findCueMatches(cues, "   ")).toEqual([]);
  });

  // Guards the class of bug that once hid half an episode's lines: matches
  // must come from the whole list, not some truncated prefix of it.
  it("finds matches anywhere in a long list", () => {
    const many = Array.from({ length: 1500 }, (_, i) => cue(`line ${i}`));
    many[1499] = cue("the very last line");
    expect(findCueMatches(many, "very last")).toEqual([1499]);
  });
});

describe("stepMatch", () => {
  it("moves forward and back", () => {
    expect(stepMatch(0, 1, 3)).toBe(1);
    expect(stepMatch(2, -1, 3)).toBe(1);
  });

  it("wraps past either end", () => {
    expect(stepMatch(2, 1, 3)).toBe(0);
    expect(stepMatch(0, -1, 3)).toBe(2);
  });

  it("starts from the first match going forward, the last going back, when unset", () => {
    expect(stepMatch(-1, 1, 3)).toBe(0);
    expect(stepMatch(-1, -1, 3)).toBe(2);
  });

  it("recovers from an index left over from a longer match list", () => {
    expect(stepMatch(7, 1, 3)).toBe(0);
  });

  it("returns -1 when there are no matches", () => {
    expect(stepMatch(0, 1, 0)).toBe(-1);
  });
});
