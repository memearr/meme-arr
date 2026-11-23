// Every call the web app makes to the backend, checked for the address and verb it uses.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { api } from "./api.js";

let calls;
let unauthorized;

function reply(status = 200, body = { ok: true }, statusText = "") {
  return Promise.resolve(
    new Response(body == null ? null : JSON.stringify(body), {
      status,
      statusText,
      headers: { "Content-Type": "application/json" },
    })
  );
}

beforeEach(() => {
  calls = [];
  const win = new EventTarget();
  unauthorized = vi.fn();
  win.addEventListener("memearr:unauthorized", unauthorized);
  vi.stubGlobal("window", win);
  vi.stubGlobal(
    "fetch",
    vi.fn((url, opts) => {
      calls.push([url, opts || {}]);
      return reply();
    })
  );
});

afterEach(() => vi.unstubAllGlobals());

const file = { name: "my song.mp3" };

// [method, args, expected URL, expected verb]
const TABLE = [
  ["authStatus", [], "/api/auth/status", undefined],
  ["authSetup", ["pw"], "/api/auth/setup", "POST"],
  ["authLogin", ["pw"], "/api/auth/login", "POST"],
  ["authLogout", [], "/api/auth/logout", "POST"],
  ["authChangePassword", ["a", "b"], "/api/auth/change-password", "POST"],
  ["getAuthentikSettings", [], "/api/settings/authentik", undefined],
  ["putAuthentikSettings", [{ a: 1 }], "/api/settings/authentik", "PUT"],
  ["deleteAuthentikSettings", [], "/api/settings/authentik", "DELETE"],
  ["connectionInfo", [], "/api/system/connection", undefined],
  ["getBazarrSettings", [], "/api/settings/bazarr", undefined],
  ["putBazarrSettings", [{}], "/api/settings/bazarr", "PUT"],
  ["deleteBazarrSettings", [], "/api/settings/bazarr", "DELETE"],
  ["testBazarr", [], "/api/settings/bazarr/test", "POST"],
  ["getNavidromeSettings", [], "/api/settings/navidrome", undefined],
  ["putNavidromeSettings", [{}], "/api/settings/navidrome", "PUT"],
  ["deleteNavidromeSettings", [], "/api/settings/navidrome", "DELETE"],
  ["testNavidrome", [], "/api/settings/navidrome/test", "POST"],
  ["musicSources", [], "/api/music/sources", undefined],
  ["songUploads", [], "/api/music/uploads", undefined],
  ["uploadSong", [file], "/api/music/uploads?name=my%20song.mp3", "POST"],
  ["musicSearch", ["a b"], "/api/music/search?q=a%20b&limit=20", undefined],
  ["musicSearch", ["a", 5], "/api/music/search?q=a&limit=5", undefined],
  ["musicPeaks", ["x/y"], "/api/music/x%2Fy/peaks?bins=600", undefined],
  ["musicPeaks", ["x", 10], "/api/music/x/peaks?bins=10", undefined],
  ["clipLoudness", ["7", 1.4, 5.6], "/api/loudness/7?start=1&end=6", undefined],
  ["clipWaveform", ["7", 1, 2], "/api/waveform/7?start=1&end=2&bins=400", undefined],
  ["clipWaveform", ["7", 1, 2, 50], "/api/waveform/7?start=1&end=2&bins=50", undefined],
  ["rotateBazarrHook", [], "/api/settings/bazarr/rotate-hook", "POST"],
  ["itemBazarr", ["7"], "/api/media/7/bazarr", undefined],
  ["bazarrSearch", ["7"], "/api/media/7/bazarr/search", "POST"],
  ["bazarrShift", ["7", 250], "/api/media/7/bazarr/shift", "POST"],
  ["bazarrSync", ["7"], "/api/media/7/bazarr/sync", "POST"],
  ["getPlexLoginSettings", [], "/api/settings/plex-login", undefined],
  ["deletePlexLoginSettings", [], "/api/settings/plex-login", "DELETE"],
  ["searchDialogue", ["hi"], "/api/search?q=hi&offset=0&limit=50", undefined],
  [
    "searchDialogue",
    ["hi", { libraries: ["TV", "A&B"], offset: 5, limit: 10 }],
    "/api/search?q=hi&offset=5&limit=10&library=TV&library=A%26B",
    undefined,
  ],
  ["searchTitle", ["hi"], "/api/search/title?q=hi&offset=0&limit=50", undefined],
  ["searchTitle", ["hi", { libraries: ["TV"] }], "/api/search/title?q=hi&offset=0&limit=50&library=TV", undefined],
  ["media", ["a/b"], "/api/media/a%2Fb", undefined],
  ["videoInfo", ["7"], "/api/media/7/video-info", undefined],
  ["mediaSubtitles", ["7", 1, 2], "/api/media/7/subtitles?start=1&end=2", undefined],
  ["indexStatus", [], "/api/index/status", undefined],
  ["reindex", [], "/api/index/reindex", "POST"],
  ["listTasks", [], "/api/tasks", undefined],
  ["getLogLevel", [], "/api/settings/log-level", undefined],
  ["setLogLevel", ["debug"], "/api/settings/log-level", "PUT"],
  ["browseDirs", [], "/api/system/browse", undefined],
  ["browseDirs", ["/a b"], "/api/system/browse?path=%2Fa%20b", undefined],
  ["getTrustedProxies", [], "/api/settings/trusted-proxies", undefined],
  ["setTrustedProxies", ["10.0.0.0/8"], "/api/settings/trusted-proxies", "PUT"],
  ["apiKeyStatus", [], "/api/settings/api-key", undefined],
  ["generateApiKey", [], "/api/settings/api-key", "POST"],
  ["revokeApiKey", [], "/api/settings/api-key", "DELETE"],
  ["listBackups", [], "/api/backups", undefined],
  ["createBackup", [], "/api/backups", "POST"],
  ["restoreBackup", ['{"a":1}'], "/api/backups/restore", "POST"],
  ["health", [], "/api/health", undefined],
  ["recheckHealth", [], "/api/health/check", "POST"],
  ["runTask", ["a b"], "/api/tasks/a%20b/run", "POST"],
  ["updateTask", ["t", { every: 1 }], "/api/tasks/t", "PUT"],
  ["wanted", [], "/api/wanted?q=&offset=0&limit=50", undefined],
  ["wanted", ["x", ["TV"], 2, 5], "/api/wanted?q=x&offset=2&limit=5&library=TV", undefined],
  ["reindexItem", ["7"], "/api/media/7/reindex", "POST"],
  ["subtitleDiagnose", ["7"], "/api/media/7/subtitles/diagnose", undefined],
  ["version", [], "/api/version", undefined],
  ["libraryStats", [], "/api/libraries/stats", undefined],
  ["activityLog", [], "/api/activity/log?limit=200", undefined],
  ["activityLog", [5], "/api/activity/log?limit=5", undefined],
  ["clearActivityLog", [], "/api/activity/log", "DELETE"],
  ["systemStorage", [], "/api/system/storage", undefined],
  ["listExports", [], "/api/exports", undefined],
  ["vacuumDatabase", [], "/api/system/vacuum", "POST"],
  ["clearPreviewCache", [], "/api/system/clear-preview-cache", "POST"],
  ["clearExports", [], "/api/system/clear-exports", "POST"],
  ["libraryMissing", ["TV Shows"], "/api/libraries/missing?library=TV%20Shows", undefined],
  ["libraryBrowse", ["q"], "/api/library/browse?q=q&offset=0&limit=60&status=all", undefined],
  [
    "libraryBrowse",
    ["q", 3, 9, ["TV"], "missing"],
    "/api/library/browse?q=q&offset=3&limit=9&status=missing&library=TV",
    undefined,
  ],
  ["showEpisodes", ["The Show"], "/api/library/episodes?show=The%20Show", undefined],
  ["startExport", [{ a: 1 }], "/api/export", "POST"],
  ["startSequenceExport", [{ a: 1 }], "/api/sequence/export", "POST"],
  ["saveFrame", [{ a: 1 }], "/api/still", "POST"],
  ["job", ["j/1"], "/api/jobs/j%2F1", undefined],
  ["getSettings", [], "/api/settings", undefined],
  ["saveSettings", [{ a: 1 }], "/api/settings", "PUT"],
  ["deleteSettings", [], "/api/settings", "DELETE"],
  ["testSettings", [{ a: 1 }], "/api/settings/test", "POST"],
  ["testSettings", [], "/api/settings/test", "POST"],
  ["track", [{ x: 1 }], "/api/track", "POST"],
  ["overlays", [], "/api/overlays", undefined],
  ["uploadOverlay", [{ name: "o.png" }], "/api/overlays?name=o.png", "POST"],
  ["fonts", [], "/api/fonts", undefined],
  ["uploadFont", [{ name: "f.ttf" }], "/api/fonts?name=f.ttf", "POST"],
];

describe("api addresses and verbs", () => {
  it.each(TABLE)("%s", async (name, args, url, verb) => {
    await api[name](...args);
    expect(calls).toHaveLength(1);
    expect(calls[0][0]).toBe(url);
    expect(calls[0][1].method).toBe(verb);
  });

  it("sends JSON bodies as JSON", async () => {
    await api.authChangePassword("a", "b");
    expect(JSON.parse(calls[0][1].body)).toEqual({ current_password: "a", new_password: "b" });
    await api.bazarrShift("7", 250);
    expect(JSON.parse(calls[1][1].body)).toEqual({ offset_ms: 250 });
    await api.setTrustedProxies("x");
    expect(JSON.parse(calls[2][1].body)).toEqual({ trusted_proxies: "x" });
    await api.testSettings();
    expect(JSON.parse(calls[3][1].body)).toEqual({});
    await api.restoreBackup("raw text");
    expect(calls[4][1].body).toBe("raw text");
  });

  it("uploads a file as raw bytes", async () => {
    await api.uploadSong(file);
    expect(calls[0][1].body).toBe(file);
    expect(calls[0][1].headers["Content-Type"]).toBe("application/octet-stream");
  });
});

describe("log queries", () => {
  it("sends the level and filter, and the cursor only when there is one", async () => {
    await api.getLogs({ level: "info" });
    await api.getLogs({ level: "warn", q: "boom", after: 0 });
    expect(calls[0][0]).toBe("/api/logs?level=info&q=");
    expect(calls[1][0]).toBe("/api/logs?level=warn&q=boom&after=0");
  });
});

describe("picture addresses", () => {
  it("builds still, filmstrip and preview addresses", () => {
    expect(api.frameUrl("7", 3.6)).toBe("/api/frame/7?t=4&width=320");
    expect(api.frameUrl("7", 1, 100)).toBe("/api/frame/7?t=1&width=100");
    expect(api.filmstripUrl("7", 0.4, 9.6, 8)).toBe("/api/filmstrip/7?start=0&end=10&count=8&height=60");
    expect(api.filmstripUrl("7", 0, 1, 2, 30)).toContain("height=30");
  });

  it("leaves the optional preview settings out until they are set", () => {
    const plain = api.previewUrl("a/b", { start: 1, end: 2, width: 480 });
    expect(plain).toBe("/api/preview/a%2Fb?start=1&end=2&width=480");
    const full = api.previewUrl("7", {
      start: 1,
      end: 2,
      width: 480,
      fps: 12,
      captionMode: "both",
      top: "T",
      bottom: "B",
      fontSize: 30,
      font: "Impact",
      fontScale: 120,
      offsetMs: -200,
      editedCues: [{ text: "x" }],
      look: { contrast: 1.2 },
    });
    const q = new URL(full, "http://x").searchParams;
    expect(q.get("fps")).toBe("12");
    expect(q.get("caption_mode")).toBe("both");
    expect(q.get("top")).toBe("T");
    expect(q.get("bottom")).toBe("B");
    expect(q.get("font_size")).toBe("30");
    expect(q.get("font")).toBe("Impact");
    expect(q.get("font_scale")).toBe("120");
    expect(q.get("offset_ms")).toBe("-200");
    expect(JSON.parse(q.get("cues_json"))).toEqual([{ text: "x" }]);
    expect(JSON.parse(q.get("look_json"))).toEqual({ contrast: 1.2 });
    // A font scale of 100 is the default and isn't sent.
    expect(api.previewUrl("7", { start: 1, end: 2, width: 3, fontScale: 100 })).not.toContain("font_scale");
  });
});

describe("deleting uploads", () => {
  it.each([
    ["deleteSongUpload", "/api/music/uploads/a%2Fb"],
    ["deleteOverlay", "/api/overlays/a%2Fb"],
    ["deleteFont", "/api/fonts/a%2Fb"],
  ])("%s calls DELETE and accepts success", async (name, url) => {
    await expect(api[name]("a/b")).resolves.toBeUndefined();
    expect(calls[0]).toEqual([url, { method: "DELETE" }]);
  });

  it.each(["deleteSongUpload", "deleteOverlay", "deleteFont"])("%s fails with the status text", async (name) => {
    fetch.mockImplementationOnce(() => reply(500, null, "Server Error"));
    await expect(api[name]("x")).rejects.toThrow("Server Error");
  });

  it.each(["deleteSongUpload", "deleteOverlay", "deleteFont"])("%s signals a lost session", async (name) => {
    fetch.mockImplementationOnce(() => reply(401, null, "Unauthorized"));
    await expect(api[name]("x")).rejects.toThrow();
    expect(unauthorized).toHaveBeenCalledTimes(1);
  });

  it("deleteFont passes the server's own message through", async () => {
    fetch.mockImplementationOnce(() => reply(409, { error: "font is in use" }));
    await expect(api.deleteFont("x")).rejects.toThrow("font is in use");
    fetch.mockImplementationOnce(() => Promise.resolve(new Response("nope", { status: 500, statusText: "Boom" })));
    await expect(api.deleteFont("x")).rejects.toThrow("Boom");
  });
});

describe("fetching a picture or video as a blob", () => {
  beforeEach(() => {
    URL.createObjectURL = vi.fn(() => "blob:made");
  });

  it("returns an object address for the bytes", async () => {
    fetch.mockImplementationOnce(() => Promise.resolve(new Response(new Uint8Array([1, 2, 3]))));
    await expect(api.fetchBlobUrl("/api/preview/7")).resolves.toBe("blob:made");
  });

  it("refuses an empty body", async () => {
    fetch.mockImplementationOnce(() => Promise.resolve(new Response(new Uint8Array([]))));
    await expect(api.fetchBlobUrl("/x")).rejects.toThrow(/empty preview/);
  });

  it("surfaces the server's error text, or the status text when there is none", async () => {
    fetch.mockImplementationOnce(() => reply(422, { error: "bad window" }));
    await expect(api.fetchBlobUrl("/x")).rejects.toThrow("bad window");
    fetch.mockImplementationOnce(() => Promise.resolve(new Response("<html>", { status: 502, statusText: "Bad Gateway" })));
    await expect(api.fetchBlobUrl("/x")).rejects.toThrow("Bad Gateway");
  });
});
