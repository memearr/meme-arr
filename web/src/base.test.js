import { describe, expect, it } from "vitest";
import { getBase, withBase } from "./base.js";

describe("withBase", () => {
  it("is a no-op at the root", () => {
    expect(withBase("/api/health", "")).toBe("/api/health");
  });

  it("prefixes root-relative paths, including query strings", () => {
    expect(withBase("/api/health", "/meme-arr")).toBe("/meme-arr/api/health");
    expect(withBase("/api/logs?level=info&q=a", "/meme-arr")).toBe("/meme-arr/api/logs?level=info&q=a");
  });

  it("leaves absolute, protocol-relative, and blob URLs alone", () => {
    for (const u of ["https://plex.tv/x", "//cdn.example/x", "blob:http://h/abc", "relative/path"]) {
      expect(withBase(u, "/meme-arr")).toBe(u);
    }
  });

  // The server already prefixes download links it builds; applying the base
  // to one of those must not double it.
  it("doesn't double-prefix", () => {
    expect(withBase("/meme-arr/api/download/x.gif", "/meme-arr")).toBe("/meme-arr/api/download/x.gif");
    expect(withBase("/meme-arr", "/meme-arr")).toBe("/meme-arr");
  });

  // "/meme-arrow" must not be mistaken for being under "/meme-arr".
  it("only treats whole path segments as already-prefixed", () => {
    expect(withBase("/meme-arrow/x", "/meme-arr")).toBe("/meme-arr/meme-arrow/x");
  });
});

describe("getBase", () => {
  it("reads the injected value, defaulting to the root", () => {
    expect(getBase({ __MEMEARR_BASE__: "/x" })).toBe("/x");
    expect(getBase({})).toBe("");
    expect(getBase(undefined)).toBe("");
  });
});
