import { describe, expect, it } from "vitest";
import { focusForItem, focusForLibrary, focusForShow, resolveFocus } from "./wantedFocus.js";

const ep = { rating_key: "e1", title: "Tideline", show: "The Lantern Cove", library: "TV Shows" };

describe("focus builders", () => {
  it("an item filters by its title, selects its library and names the row to open", () => {
    expect(focusForItem(ep)).toMatchObject({ query: "Tideline", libraries: ["TV Shows"], openKey: "e1" });
    expect(focusForItem({ rating_key: "m1", title: "Dune" })).toMatchObject({ libraries: [], openKey: "m1" });
  });
  it("a show filters by name and opens the row only if one episode is missing", () => {
    expect(focusForShow("Orbital Diner")).toMatchObject({ query: "Orbital Diner", single: true, openKey: null });
  });
  it("a library selects just that library", () => {
    expect(focusForLibrary("Movies")).toMatchObject({ query: "", libraries: ["Movies"], openKey: null });
  });
});

describe("resolveFocus", () => {
  const items = [{ rating_key: "e1" }, { rating_key: "e2" }];
  it("does nothing without a focus", () => {
    expect(resolveFocus(null, items)).toEqual({ open: null, note: null });
  });
  it("opens the wanted item when it is in the list", () => {
    expect(resolveFocus(focusForItem(ep), items)).toEqual({ open: "e1", note: null });
  });
  it("says so when the item isn't wanted (it has subtitles, or is unreachable)", () => {
    const r = resolveFocus(focusForItem({ ...ep, rating_key: "gone" }), items);
    expect(r.open).toBeNull();
    expect(r.note).toContain("The Lantern Cove · Tideline");
    expect(r.note).toMatch(/already have subtitles|can't be reached/);
  });
  it("a show with exactly one missing episode opens it; with several it just lists them", () => {
    expect(resolveFocus(focusForShow("X"), [{ rating_key: "only" }])).toEqual({ open: "only", note: null });
    expect(resolveFocus(focusForShow("X"), items)).toEqual({ open: null, note: null });
    expect(resolveFocus(focusForShow("X"), [])).toEqual({ open: null, note: null });
  });
  it("a library focus never opens a row", () => {
    expect(resolveFocus(focusForLibrary("TV"), [{ rating_key: "only" }])).toEqual({ open: null, note: null });
  });
});
