import { describe, expect, it } from "vitest";
import {
  candidateBadges,
  capabilityView,
  DEFAULT_LANGUAGE,
  emptyMessage,
  fixFor,
  detailEntries,
  findingIcon,
  folderFileNote,
  followUpMessage,
  serverStreamText,
  worstLevel,
  requestCounts,
  requestItemTitle,
  requestStatus,
  requestedSubtitleText,
  SLOW_REQUEST_SECS,
  humanizeKey,
  popularityText,
  reindexView,
  unreachableNote,
  wantedTitle,
  LANGUAGES,
  languageLabel,
  loadLanguage,
  saveLanguage,
  sortCandidates,
  validLanguage,
} from "./subtitles.js";

describe("capabilityView", () => {
  it("is neutral while loading", () => {
    expect(capabilityView(null)).toMatchObject({ ok: false, title: "Checking…" });
  });

  it("describes a checked Plex server", () => {
    const v = capabilityView({
      supported: true,
      server: "plex",
      version: "1.41.3.9314-a0bfb8370",
      version_checked: true,
      minimum_version: "1.13.8.5395",
    });
    expect(v.ok).toBe(true);
    expect(v.title).toContain("Plex 1.41.3.9314");
    expect(v.detail).toContain("1.13.8.5395");
  });

  it("is honest that an unreadable Plex version was not checked", () => {
    const v = capabilityView({ supported: true, server: "plex", version: "weird", version_checked: false });
    expect(v.ok).toBe(true);
    expect(v.detail).toMatch(/attempted anyway/);
  });

  it("warns that Jellyfin needs a provider plugin", () => {
    const v = capabilityView({ supported: true, server: "jellyfin", version: "10.10.3", needs_provider_plugin: true });
    expect(v.title).toContain("Jellyfin 10.10.3");
    expect(v.detail).toMatch(/provider/i);
  });

  it("explains an unavailable server with a fix", () => {
    const v = capabilityView({ supported: false, reason: "Plex 1.9.0 is too old for subtitle search; it needs 1.13.8.5395 or newer" });
    expect(v.ok).toBe(false);
    expect(v.detail).toContain("too old");
    expect(v.fix).toMatch(/Update Plex/);
  });
});

describe("fixFor", () => {
  it.each([
    ["Plex is not configured — set it up in Settings", /Settings/],
    ["Couldn't read the Plex server version: boom", /reachable/],
    ["Online subtitle search is only available for Plex so far", /isn't supported/],
    ["something else entirely", /Health/],
  ])("%s", (reason, re) => {
    expect(fixFor(reason)).toMatch(re);
  });
});

describe("sortCandidates", () => {
  const c = (key, extra = {}) => ({ key, ...extra });

  it("puts hash matches first, then by downloads, then server order", () => {
    const out = sortCandidates([
      c("a"),
      c("b", { downloads: 5 }),
      c("c", { downloads: 500 }),
      c("d", { hash_match: true, downloads: 1 }),
      c("e"),
    ]);
    expect(out.map((x) => x.key)).toEqual(["d", "c", "b", "a", "e"]);
  });

  it("keeps the server's order when nothing distinguishes candidates (Plex)", () => {
    const input = [c("x"), c("y"), c("z")];
    expect(sortCandidates(input).map((x) => x.key)).toEqual(["x", "y", "z"]);
  });

  it("doesn't mutate its input", () => {
    const input = [c("a"), c("b", { hash_match: true })];
    sortCandidates(input);
    expect(input.map((x) => x.key)).toEqual(["a", "b"]);
  });
});

describe("candidateBadges", () => {
  it("lists only the flags that are set", () => {
    expect(candidateBadges({})).toEqual([]);
    expect(candidateBadges({ hash_match: true, hearing_impaired: true, forced: true })).toEqual([
      "Hash match",
      "SDH",
      "Forced",
    ]);
    expect(candidateBadges({ forced: true })).toEqual(["Forced"]);
  });
});

describe("messages", () => {
  it("prefers the server's note on an empty result", () => {
    expect(emptyMessage("Install a provider plugin.", "en")).toBe("Install a provider plugin.");
    expect(emptyMessage(undefined, "fr")).toBe("No French subtitles found.");
  });

  it("follow-up names the right server and tells you to index the item", () => {
    expect(followUpMessage("jellyfin")).toMatch(/^Jellyfin .*index this item/);
    expect(followUpMessage("plex")).toMatch(/^Plex .*index this item/);
  });
});

describe("language choice", () => {
  it("only offers languages the backend can map, with unique codes", () => {
    const codes = LANGUAGES.map(([c]) => c);
    expect(new Set(codes).size).toBe(codes.length);
    for (const c of codes) expect(c).toMatch(/^[a-z]{2}$/);
    expect(languageLabel("pt")).toBe("Portuguese");
    expect(languageLabel("zz")).toBe("zz");
  });

  it("falls back to English for anything unknown", () => {
    expect(validLanguage("fr")).toBe("fr");
    expect(validLanguage("xx")).toBe(DEFAULT_LANGUAGE);
    expect(validLanguage(null)).toBe(DEFAULT_LANGUAGE);
  });

  it("round-trips through storage and survives broken storage", () => {
    const store = new Map();
    const storage = { getItem: (k) => store.get(k) ?? null, setItem: (k, v) => store.set(k, v) };
    expect(loadLanguage(storage)).toBe("en");
    saveLanguage(storage, "ja");
    expect(loadLanguage(storage)).toBe("ja");
    const broken = {
      getItem() {
        throw new Error("blocked");
      },
      setItem() {
        throw new Error("blocked");
      },
    };
    expect(loadLanguage(broken)).toBe("en");
    expect(() => saveLanguage(broken, "ja")).not.toThrow();
    expect(loadLanguage(undefined)).toBe("en");
  });
});

describe("reindexView", () => {
  it("celebrates real lines, with correct plural", () => {
    expect(reindexView({ status: "indexed", lines: 1204 })).toMatchObject({ ok: true, done: true });
    expect(reindexView({ status: "indexed", lines: 1204 }).message).toContain("1,204 subtitle lines");
    expect(reindexView({ status: "indexed", lines: 1 }).message).toContain("1 subtitle line —");
  });

  it("is honest when it indexed but found no text", () => {
    const v = reindexView({ status: "indexed", lines: 0 });
    expect(v).toMatchObject({ ok: false, done: false });
    expect(v.message).toMatch(/no usable text/);
  });

  it("explains an unchanged item: still fetching, or saved elsewhere", () => {
    const v = reindexView({ status: "unchanged", lines: 0 });
    expect(v.done).toBe(false);
    expect(v.message).toMatch(/still be fetching/);
    expect(v.message).toMatch(/somewhere this app can't read/);
  });

  it("is empty for no result", () => {
    expect(reindexView(null).message).toBe("");
  });
});

describe("wanted list text", () => {
  it("titles episodes and movies", () => {
    expect(wantedTitle({ show: "Show A", season: 1, episode: 2, title: "Pilot" })).toEqual({
      main: "Show A S01E02",
      sub: "Pilot",
    });
    expect(wantedTitle({ show: "Show A", title: "Special" })).toEqual({ main: "Show A", sub: "Special" });
    expect(wantedTitle({ title: "Dune", year: 2021 })).toEqual({ main: "Dune (2021)", sub: null });
    expect(wantedTitle({ title: "Untitled" })).toEqual({ main: "Untitled", sub: null });
  });

  it("only mentions unreachable items when there are some", () => {
    expect(unreachableNote(0)).toBeNull();
    expect(unreachableNote(1)).toMatch(/^1 more item isn't listed/);
    expect(unreachableNote(1200)).toMatch(/^1,200 more items aren't listed/);
  });
});

describe("result details", () => {
  it("labels the match kind per server and flags machine translation", () => {
    expect(candidateBadges({ hash_match: true }, "plex")).toEqual(["Perfect match"]);
    expect(candidateBadges({ hash_match: true }, "jellyfin")).toEqual(["Hash match"]);
    expect(candidateBadges({ hash_match: true })).toEqual(["Hash match"]);
    expect(candidateBadges({ ai_translated: true, machine_translated: true })).toEqual([
      "AI translated",
      "Machine translated",
    ]);
  });

  it("describes popularity in each server's own terms", () => {
    expect(popularityText({ downloads: 5000 })).toBe("5,000 downloads");
    expect(popularityText({ score: 4200 })).toBe("score 4,200");
    expect(popularityText({ downloads: 0 })).toBe("0 downloads");
    expect(popularityText({})).toBeNull();
  });

  it("ranks a Plex perfect match first, then by score", () => {
    const out = sortCandidates([
      { key: "a", score: 10 },
      { key: "b", score: 900 },
      { key: "c", hash_match: true, score: 1 },
      { key: "d" },
    ]);
    expect(out.map((x) => x.key)).toEqual(["c", "b", "a", "d"]);
  });

  it("makes server attribute names readable", () => {
    expect(humanizeKey("canAutoSync")).toBe("Can auto sync");
    expect(humanizeKey("sourceKey")).toBe("Source key");
    expect(humanizeKey("ThreeLetterISOLanguageName")).toBe("Three letter ISO language name");
    expect(humanizeKey("CommunityRating")).toBe("Community rating");
    expect(humanizeKey("languageCode")).toBe("Language code");
  });

  it("lists details alphabetically, and tolerates none", () => {
    expect(detailEntries({ sourceKey: "x:1", canAutoSync: "1" })).toEqual([
      { label: "Can auto sync", value: "1" },
      { label: "Source key", value: "x:1" },
    ]);
    expect(detailEntries(undefined)).toEqual([]);
    expect(detailEntries({})).toEqual([]);
  });
});

describe("subtitle requests", () => {
  const req = (over) => ({ id: 1, rating_key: "42", state: "waiting", lines: 0, requested_at: 100, subtitle: "Rel.WEB", ...over });

  it("shows an indexed request with its line count and plural", () => {
    expect(requestStatus(req({ state: "indexed", lines: 1204 }), 5)).toMatchObject({
      label: "Indexed · 1,204 lines",
      tone: "done",
      canReindex: true,
    });
    expect(requestStatus(req({ state: "indexed", lines: 1 }), 5).label).toBe("Indexed · 1 line");
  });

  it("a waiting request changes its advice once it has waited a while", () => {
    const fresh = requestStatus(req(), 30);
    const slow = requestStatus(req(), SLOW_REQUEST_SECS);
    expect(fresh).toMatchObject({ label: "Waiting", tone: "queued", canReindex: true });
    expect(fresh.hint).toMatch(/moment/);
    expect(slow.hint).toMatch(/Diagnose/);
    expect(requestStatus(req(), SLOW_REQUEST_SECS - 1).hint).toMatch(/moment/); // one second short of "slow"
  });

  it("a request whose item is gone can't be re-indexed", () => {
    const s = requestStatus(req({ state: "item_missing" }), 5);
    expect(s).toMatchObject({ tone: "failed", canReindex: false });
    expect(s.hint).toMatch(/no longer in the index/);
  });

  it("titles the item and describes what was requested", () => {
    expect(requestItemTitle(req({ show: "Show A", season: 1, episode: 2, item_title: "Pilot" }))).toEqual({
      main: "Show A S01E02",
      sub: "Pilot",
    });
    expect(requestItemTitle(req({ item_title: "Dune", year: 2021 }))).toEqual({ main: "Dune (2021)", sub: null });
    // item not in the index any more: fall back to the key rather than "undefined"
    expect(requestItemTitle(req({ item_title: null })).main).toBe("42");
    expect(requestedSubtitleText(req({ provider: "OpenSubtitles", language: "en" }))).toBe("Rel.WEB · OpenSubtitles · en");
    expect(requestedSubtitleText(req({ provider: null, language: null }))).toBe("Rel.WEB");
  });

  it("counts waiting and indexed for the tab and the clear button", () => {
    expect(requestCounts([])).toEqual({ total: 0, waiting: 0, indexed: 0 });
    expect(
      requestCounts([req({ state: "waiting" }), req({ state: "indexed" }), req({ state: "indexed" }), req({ state: "item_missing" })])
    ).toEqual({ total: 4, waiting: 1, indexed: 2 });
  });
});

describe("diagnosis helpers", () => {
  it("has a distinct icon per level and a fallback", () => {
    const icons = ["ok", "info", "warn", "error"].map(findingIcon);
    expect(new Set(icons).size).toBe(4);
    expect(findingIcon("nonsense")).toBe("•");
  });

  it("finds the most serious level", () => {
    expect(worstLevel([{ level: "ok" }, { level: "warn" }, { level: "info" }])).toBe("warn");
    expect(worstLevel([{ level: "info" }, { level: "error" }])).toBe("error");
    expect(worstLevel([{ level: "ok" }])).toBe("ok");
    expect(worstLevel([])).toBe("info");
    expect(worstLevel(undefined)).toBe("info");
  });

  it("explains what the indexer makes of each file", () => {
    expect(folderFileNote({ picked: true, matches_video: true, indexable: true })).toMatch(/used/);
    expect(folderFileNote({ picked: false, matches_video: false, indexable: true })).toMatch(/name doesn't start/);
    expect(folderFileNote({ picked: false, matches_video: true, indexable: false })).toMatch(/image-based/);
    expect(folderFileNote({ picked: false, matches_video: true, indexable: true })).toMatch(/not chosen/);
    // a mismatched, image-based file is explained by the name first (the more fixable thing)
    expect(folderFileNote({ picked: false, matches_video: false, indexable: false })).toMatch(/name doesn't start/);
  });

  it("describes a server-listed subtitle", () => {
    expect(serverStreamText({ name: "English (SRT)", language: "English", format: "srt", external: true })).toBe(
      "English (SRT) · English · srt · external"
    );
    expect(serverStreamText({ name: "Subtitle", external: false })).toBe("Subtitle · embedded");
  });
});
