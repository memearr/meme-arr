import { describe, expect, it } from "vitest";
import { addFilter, buildQuery, describeFilter, filterToken, makeFilter, removeFilterAt, typedOnly } from "./searchScope.js";

describe("makeFilter", () => {
  it("builds each kind and rejects what isn't usable", () => {
    expect(makeFilter("show", " Orbital Diner ")).toEqual({ field: "show", value: "Orbital Diner", negate: false });
    expect(makeFilter("episode", "docking", true)).toEqual({ field: "episode", value: "docking", negate: true });
    expect(makeFilter("season", "02")).toEqual({ field: "season", value: "2", negate: false });
    expect(makeFilter("only-movies", "")).toEqual({ field: "movie", value: "", negate: false });
    expect(makeFilter("only-episodes", "", true)).toEqual({ field: "episode", value: "", negate: true });
    expect(makeFilter("show", "  ")).toBeNull();
    expect(makeFilter("season", "two")).toBeNull();
    expect(makeFilter("nope", "x")).toBeNull();
  });
  it("drops quotes so a value can't break out of the operator", () => {
    expect(makeFilter("show", 'The "Big" Show').value).toBe("The Big Show");
  });
});

describe("filterToken / buildQuery", () => {
  it("quotes values with spaces and marks exclusions", () => {
    expect(filterToken({ field: "show", value: "Lantern Cove", negate: false })).toBe('show:"Lantern Cove"');
    expect(filterToken({ field: "episode", value: "pilot", negate: true })).toBe("-ep:pilot");
    expect(filterToken({ field: "season", value: "2", negate: false })).toBe("season:2");
    expect(filterToken({ field: "movie", value: "", negate: false })).toBe("movie:");
    expect(filterToken({ field: "episode", value: "", negate: true })).toBe("-ep:");
  });
  it("puts the text first and each filter after it", () => {
    const scope = [makeFilter("show", "Lantern Cove"), makeFilter("episode", "gull", true)];
    expect(buildQuery("  tide for you ", scope)).toBe('tide for you show:"Lantern Cove" -ep:gull');
    expect(buildQuery("", scope)).toBe('show:"Lantern Cove" -ep:gull');
    expect(buildQuery("knock", [])).toBe("knock");
    expect(buildQuery("", [])).toBe("");
  });
});

describe("describeFilter matches the server's wording", () => {
  it("reads like src/querysyntax.rs describe()", () => {
    expect(describeFilter({ field: "show", value: "Orbital Diner", negate: false })).toBe("show: Orbital Diner");
    expect(describeFilter({ field: "episode", value: "pilot", negate: true })).toBe("not episode: pilot");
    expect(describeFilter({ field: "season", value: "2", negate: false })).toBe("season: 2");
    expect(describeFilter({ field: "movie", value: "", negate: false })).toBe("movies only");
    expect(describeFilter({ field: "episode", value: "", negate: true })).toBe("not episodes");
  });
});

describe("scope editing", () => {
  const a = makeFilter("show", "Lantern Cove");
  it("adds a filter once, ignoring case", () => {
    expect(addFilter([], a)).toEqual([a]);
    expect(addFilter([a], makeFilter("show", "lantern cove"))).toEqual([a]);
    expect(addFilter([a], makeFilter("show", "Lantern Cove", true))).toHaveLength(2);
    expect(addFilter([a], null)).toEqual([a]);
  });
  it("removes by position", () => {
    const b = makeFilter("season", "2");
    expect(removeFilterAt([a, b], 0)).toEqual([b]);
    expect(removeFilterAt([a, b], 1)).toEqual([a]);
  });
});

describe("typedOnly", () => {
  it("leaves out what the chips already show, keeping operators typed in the box", () => {
    const scope = [makeFilter("show", "Lantern Cove")];
    expect(typedOnly(["show: Lantern Cove", "without “walter”"], scope)).toEqual(["without “walter”"]);
    expect(typedOnly(["show: Lantern Cove"], scope)).toEqual([]);
    expect(typedOnly(["show: Lantern Cove", "show: Lantern Cove"], scope)).toEqual(["show: Lantern Cove"]);
    expect(typedOnly(["season: 2"], [])).toEqual(["season: 2"]);
    expect(typedOnly(undefined, scope)).toEqual([]);
  });
});
