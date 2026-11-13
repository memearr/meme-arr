import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, describe, expect, it, vi } from "vitest";
import CopyButton from "./CopyButton.jsx";
import { copyText } from "./clipboard.js";

afterEach(() => {
  vi.unstubAllGlobals();
});

function fakeDom({ execOk = true } = {}) {
  const made = [];
  const doc = {
    body: { appendChild: (el) => made.push(el) },
    createElement: () => ({
      style: {},
      setAttribute() {},
      select() {},
      setSelectionRange() {},
      remove() {},
    }),
    execCommand: vi.fn(() => execOk),
  };
  vi.stubGlobal("document", doc);
  return doc;
}

describe("copyText", () => {
  it("uses the clipboard API on a secure page", async () => {
    const writeText = vi.fn(async () => {});
    vi.stubGlobal("navigator", { clipboard: { writeText } });
    vi.stubGlobal("window", { isSecureContext: true });
    expect(await copyText("hello")).toBe(true);
    expect(writeText).toHaveBeenCalledWith("hello");
  });

  it("falls back to select-and-copy on plain HTTP, where navigator.clipboard is missing", async () => {
    vi.stubGlobal("navigator", {});
    vi.stubGlobal("window", { isSecureContext: false });
    const doc = fakeDom();
    expect(await copyText("hello")).toBe(true);
    expect(doc.execCommand).toHaveBeenCalledWith("copy");
  });

  it("falls back when the clipboard API refuses", async () => {
    vi.stubGlobal("navigator", { clipboard: { writeText: async () => Promise.reject(new Error("denied")) } });
    vi.stubGlobal("window", { isSecureContext: true });
    const doc = fakeDom();
    expect(await copyText("x")).toBe(true);
    expect(doc.execCommand).toHaveBeenCalled();
  });

  it("says so when nothing works", async () => {
    vi.stubGlobal("navigator", {});
    vi.stubGlobal("window", { isSecureContext: false });
    fakeDom({ execOk: false });
    expect(await copyText("x")).toBe(false);
  });
});

describe("CopyButton", () => {
  it("renders a Copy button", () => {
    expect(renderToStaticMarkup(<CopyButton text="abc" />)).toContain(">Copy<");
  });
});
