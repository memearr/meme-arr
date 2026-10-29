import { describe, expect, it } from "vitest";
import { BROWSE_STATUSES, libraryFilterLabel, libraryParams, resultCountText, toggleLibrary } from "./libraryFilter.js";

const ALL = ["Anime", "Movies", "TV"];

describe("libraryParams", () => {
  it("is empty for all libraries", () => {
    expect(libraryParams([])).toBe("");
    expect(libraryParams(undefined)).toBe("");
  });
  it("repeats the parameter, encoded, once per library", () => {
    expect(libraryParams(["TV", "4K Movies"])).toBe("&library=TV&library=4K%20Movies");
    expect(libraryParams(["A&B"])).toBe("&library=A%26B");
  });
  it("ignores blank names", () => {
    expect(libraryParams(["", " ", "TV"])).toBe("&library=TV");
  });
});

describe("libraryFilterLabel", () => {
  it("says All libraries for none or every library", () => {
    expect(libraryFilterLabel([], ALL)).toBe("All libraries");
    expect(libraryFilterLabel(ALL, ALL)).toBe("All libraries");
  });
  it("names one or two, counts more", () => {
    expect(libraryFilterLabel(["TV"], ALL)).toBe("TV");
    expect(libraryFilterLabel(["TV", "Movies"], ALL)).toBe("TV + Movies");
    expect(libraryFilterLabel(["TV", "Movies", "Anime"], [...ALL, "Docs"])).toBe("3 libraries");
  });
  it("ignores a saved library that no longer exists", () => {
    expect(libraryFilterLabel(["Gone"], ALL)).toBe("All libraries");
    expect(libraryFilterLabel(["Gone", "TV"], ALL)).toBe("TV");
  });
});

describe("toggleLibrary", () => {
  it("adds and removes", () => {
    expect(toggleLibrary([], "TV", ALL)).toEqual(["TV"]);
    expect(toggleLibrary(["TV"], "Movies", ALL)).toEqual(["TV", "Movies"]);
    expect(toggleLibrary(["TV", "Movies"], "TV", ALL)).toEqual(["Movies"]);
    expect(toggleLibrary(["TV"], "TV", ALL)).toEqual([]);
  });
  it("collapses to all when every library is ticked", () => {
    expect(toggleLibrary(["TV", "Movies"], "Anime", ALL)).toEqual([]);
  });
});

describe("resultCountText", () => {
  it("is blank with no results", () => {
    expect(resultCountText(0, false)).toBe("");
  });
  it("says when the list is cut off", () => {
    expect(resultCountText(50, true)).toBe("Showing the first 50 matches");
  });
  it("counts a complete list", () => {
    expect(resultCountText(1, false)).toBe("1 match");
    expect(resultCountText(1234, false)).toBe("1,234 matches");
  });
});

describe("library list status filter", () => {
  it("offers all, missing and fully indexed, in that order", () => {
    expect(BROWSE_STATUSES.map(([k]) => k)).toEqual(["all", "missing", "indexed"]);
  });
});
