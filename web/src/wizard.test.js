import { describe, expect, it } from "vitest";
import { buildMappingRows, locationLabel, mappingsToSave, serverPayload, suggestContainerPath } from "./wizard.js";

const dirs = ["/media/Movies", "/media/tv", "/media/d", "/media/extras"];

describe("locationLabel", () => {
  it("takes the last folder, from POSIX or Windows paths", () => {
    expect(locationLabel("/volume1/media/TV")).toBe("TV");
    expect(locationLabel("D:\\Media\\Anime\\")).toBe("Anime");
  });
  it("turns a bare Windows drive into its letter", () => {
    expect(locationLabel("D:\\")).toBe("d");
    expect(locationLabel("E:")).toBe("e");
  });
});

describe("suggestContainerPath", () => {
  it("matches the last folder name case-insensitively", () => {
    expect(suggestContainerPath("/volume1/movies", dirs)).toBe("/media/Movies");
    expect(suggestContainerPath("/volume1/TV", dirs)).toBe("/media/tv");
  });
  it("matches a Windows drive to a drive-letter folder", () => {
    expect(suggestContainerPath("D:\\", dirs)).toBe("/media/d");
    expect(suggestContainerPath("E:\\", ["/media/drive-e"])).toBe("/media/drive-e");
  });
  it("won't guess when nothing matches, or when it's ambiguous", () => {
    expect(suggestContainerPath("/volume1/documentaries", dirs)).toBe("");
    expect(suggestContainerPath("/a/tv", ["/media/tv", "/mnt/tv"])).toBe("");
    expect(suggestContainerPath("", dirs)).toBe("");
  });
  // A single-letter folder must not be mistaken for a drive on POSIX paths.
  it("doesn't treat a one-letter POSIX folder as a drive", () => {
    expect(suggestContainerPath("/volume1/d", ["/media/drived"])).toBe("");
  });
});

describe("buildMappingRows", () => {
  const libs = [
    { title: "Movies", type: "movie", locations: ["/volume1/movies"] },
    { title: "TV", type: "show", locations: ["/volume1/tv", "/volume2/tv-extra"] },
    { title: "Music", type: "artist", locations: ["/volume1/music"] },
    { title: "Kids Movies", type: "movie", locations: ["/volume1/movies"] },
  ];
  it("makes one row per distinct movie/TV folder and skips other library types", () => {
    const rows = buildMappingRows(libs, dirs);
    expect(rows.map((r) => r.plex_prefix)).toEqual(["/volume1/movies", "/volume1/tv", "/volume2/tv-extra"]);
    expect(rows[0].container_path).toBe("/media/Movies");
    expect(rows[2].container_path).toBe("");
  });
  it("keeps an existing mapping over a suggestion", () => {
    const rows = buildMappingRows(libs, dirs, [{ plex_prefix: "/volume1/tv", container_path: "/mnt/custom" }]);
    expect(rows[1].container_path).toBe("/mnt/custom");
  });
});

describe("mappingsToSave", () => {
  it("drops blank rows and trims paths", () => {
    const out = mappingsToSave([
      { library: "A", plex_prefix: "/a", container_path: " /media/a " },
      { library: "B", plex_prefix: "/b", container_path: "  " },
    ]);
    expect(out).toEqual([{ plex_prefix: "/a", container_path: "/media/a", label: "A" }]);
  });
});

describe("serverPayload", () => {
  it("sends only the chosen server's fields", () => {
    expect(serverPayload({ server: "jellyfin", url: "http://j:8096", secret: "k" })).toEqual({
      media_server: "jellyfin",
      jellyfin_url: "http://j:8096",
      jellyfin_api_key: "k",
    });
    expect(serverPayload({ server: "plex", url: "http://p:32400", secret: "t" })).toEqual({
      media_server: "plex",
      plex_url: "http://p:32400",
      plex_token: "t",
    });
  });

  it("omits a blank secret so the saved one is kept", () => {
    expect(serverPayload({ server: "plex", url: "u", secret: "" })).toEqual({
      media_server: "plex",
      plex_url: "u",
    });
  });

  it("treats anything unknown as plex", () => {
    expect(serverPayload({ server: "emby", url: "u", secret: "" }).media_server).toBe("plex");
  });
});
