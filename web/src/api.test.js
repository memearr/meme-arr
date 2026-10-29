import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { api } from "./api.js";

function respond(status, body, statusText = "") {
  return Promise.resolve(
    new Response(body == null ? null : JSON.stringify(body), {
      status,
      statusText,
      headers: { "Content-Type": "application/json" },
    })
  );
}

describe("api error handling", () => {
  let unauthorized;

  beforeEach(() => {
    const win = new EventTarget();
    unauthorized = vi.fn();
    win.addEventListener("memearr:unauthorized", unauthorized);
    vi.stubGlobal("window", win);
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("returns parsed JSON on success", async () => {
    vi.stubGlobal("fetch", vi.fn(() => respond(200, { current: "v0.13.1" })));
    await expect(api.version()).resolves.toEqual({ current: "v0.13.1" });
  });

  it("surfaces the backend's error message", async () => {
    vi.stubGlobal("fetch", vi.fn(() => respond(400, { error: "clips are limited to 60 seconds" })));
    await expect(api.version()).rejects.toThrow("clips are limited to 60 seconds");
  });

  it("falls back to the status text when the body isn't JSON", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(() => Promise.resolve(new Response("<html>oops</html>", { status: 502, statusText: "Bad Gateway" })))
    );
    await expect(api.version()).rejects.toThrow("Bad Gateway");
  });

  // A session that expires mid-use has to bounce the whole app back to the
  // login screen -- that's what this event drives.
  it("signals memearr:unauthorized on a 401 from a normal endpoint", async () => {
    vi.stubGlobal("fetch", vi.fn(() => respond(401, { error: "authentication required" })));
    await expect(api.version()).rejects.toThrow("authentication required");
    expect(unauthorized).toHaveBeenCalledTimes(1);
  });

  // A wrong password on the login form is also a 401 -- it must stay an
  // inline error, not kick the user to the screen they're already on.
  it("does not signal memearr:unauthorized for /api/auth/* endpoints", async () => {
    vi.stubGlobal("fetch", vi.fn(() => respond(401, { error: "unauthorized" })));
    await expect(api.authLogin("wrong")).rejects.toThrow("unauthorized");
    expect(unauthorized).not.toHaveBeenCalled();
  });

  it("passes the login lockout message through for the form to show", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(() => respond(429, { error: "too many failed login attempts — try again in 30s" }))
    );
    await expect(api.authLogin("x")).rejects.toThrow("try again in 30s");
    expect(unauthorized).not.toHaveBeenCalled();
  });
});

describe("subtitle search requests", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("encodes the item id and language, and sends the key as JSON", async () => {
    const calls = [];
    vi.stubGlobal(
      "fetch",
      vi.fn((url, opts) => {
        calls.push([url, opts]);
        return respond(200, { ok: true });
      })
    );
    await api.subtitleSearch("0f8f/ad", "pt-BR");
    await api.subtitleDownload("42", "a/b?c=d");
    await api.subtitleCapability();
    expect(calls[0][0]).toBe("/api/media/0f8f%2Fad/subtitles/search?language=pt-BR");
    expect(calls[1][0]).toBe("/api/media/42/subtitles/download");
    expect(calls[1][1].method).toBe("POST");
    expect(JSON.parse(calls[1][1].body)).toEqual({ key: "a/b?c=d" });
    expect(calls[2][0]).toBe("/api/subtitles/capability");
  });
});

describe("subtitle request tracking calls", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("sends the request's description with the download, and uses the right verbs for the list", async () => {
    const calls = [];
    vi.stubGlobal(
      "fetch",
      vi.fn((url, opts) => {
        calls.push([url, opts]);
        return respond(200, { ok: true });
      })
    );
    await api.subtitleDownload("42", "k", { title: "Rel", provider: "OS", language: "en" });
    await api.subtitleRequests();
    await api.deleteSubtitleRequest(7);
    await api.clearFinishedSubtitleRequests();
    expect(JSON.parse(calls[0][1].body)).toEqual({ key: "k", title: "Rel", provider: "OS", language: "en" });
    expect(calls[1][0]).toBe("/api/subtitles/requests");
    expect(calls[2]).toEqual(["/api/subtitles/requests/7", { method: "DELETE" }]);
    expect(calls[3]).toEqual(["/api/subtitles/requests?finished=true", { method: "DELETE" }]);
  });
});
