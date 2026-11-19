import { describe, expect, it } from "vitest";
import { MAX_RECENT, clearRecent, loadRecent, rememberSong } from "./recentSongs.js";

/** A stand-in for localStorage. */
function store(initial = {}) {
  const data = { ...initial };
  return {
    data,
    getItem: (k) => (k in data ? data[k] : null),
    setItem: (k, v) => {
      data[k] = String(v);
    },
  };
}
const song = (n, extra = {}) => ({ id: `s${n}`, title: `Song ${n}`, artist: "Band", album: "LP", duration_s: 100 + n, cover_art: `al-${n}`, ...extra });

describe("recent songs", () => {
  it("starts empty, and survives storage that is missing or broken", () => {
    expect(loadRecent(store())).toEqual([]);
    expect(loadRecent(null)).toEqual([]);
    expect(loadRecent(store({ "memearr.recentSongs": "{not json" }))).toEqual([]);
    expect(loadRecent(store({ "memearr.recentSongs": '{"id":"x"}' }))).toEqual([]);
  });

  it("puts the newest first and moves a repeat to the front instead of listing it twice", () => {
    const s = store();
    let list = [];
    list = rememberSong(s, list, song(1));
    list = rememberSong(s, list, song(2));
    list = rememberSong(s, list, song(1));
    expect(list.map((t) => t.id)).toEqual(["s1", "s2"]);
    expect(loadRecent(s).map((t) => t.id)).toEqual(["s1", "s2"]);
  });

  it("keeps only the latest few", () => {
    const s = store();
    let list = [];
    for (let i = 1; i <= MAX_RECENT + 3; i += 1) list = rememberSong(s, list, song(i));
    expect(list).toHaveLength(MAX_RECENT);
    expect(list[0].id).toBe(`s${MAX_RECENT + 3}`);
    expect(list.some((t) => t.id === "s1")).toBe(false);
  });

  it("keeps only what the list needs, and ignores what isn't a song", () => {
    const s = store();
    const list = rememberSong(s, [], song(1, { junk: "x", duration_s: "long" }));
    expect(list[0]).toEqual({ id: "s1", title: "Song 1", artist: "Band", album: "LP", duration_s: 0, cover_art: "al-1" });
    expect(rememberSong(s, list, { title: "no id" })).toBe(list);
    expect(rememberSong(s, list, null)).toBe(list);
  });

  it("is forgotten on request", () => {
    const s = store();
    rememberSong(s, [], song(1));
    expect(clearRecent(s)).toEqual([]);
    expect(loadRecent(s)).toEqual([]);
  });

  it("still returns the list when the browser won't store it", () => {
    const refusing = { getItem: () => null, setItem: () => { throw new Error("full"); } };
    expect(rememberSong(refusing, [], song(1)).map((t) => t.id)).toEqual(["s1"]);
  });
});
