import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import BazarrActions from "./BazarrActions.jsx";
import { BazarrSettingsPanel } from "./BazarrSettingsPanel.jsx";
import { hookUrl, missingLabel } from "./bazarr.js";

describe("hookUrl", () => {
  const base = (p) => p;
  it("is an Apprise json:// URL on this app's own address, secret last", () => {
    expect(hookUrl("http://meme-arr.lan:8080", "abc123", base)).toBe(
      "json://meme-arr.lan:8080/api/hooks/bazarr/abc123"
    );
  });
  it("uses jsons:// for https and keeps a URL base", () => {
    expect(hookUrl("https://memes.example.com", "t0k", (p) => `/meme${p}`)).toBe(
      "jsons://memes.example.com/meme/api/hooks/bazarr/t0k"
    );
  });
  it("is empty without a secret", () => {
    expect(hookUrl("http://x", null, base)).toBe("");
  });
});

describe("missingLabel", () => {
  const names = (c) => ({ en: "English", fr: "French" })[c] || c;
  it("names each language and its kind", () => {
    expect(
      missingLabel(
        [
          { code2: "en", forced: false, hi: false },
          { code2: "fr", forced: false, hi: true },
          { code2: "en", forced: true, hi: false },
        ],
        names
      )
    ).toBe("English, French (hearing impaired), English (forced)");
  });
  it("is empty for nothing", () => {
    expect(missingLabel([], names)).toBe("");
    expect(missingLabel(undefined, names)).toBe("");
  });
});

describe("the Bazarr UI", () => {
  it("draws nothing for an item until Bazarr is known to be set up", () => {
    expect(renderToStaticMarkup(<BazarrActions itemId="1" />)).toBe("");
  });
  it("offers the address, key and a Save button in Settings", () => {
    const html = renderToStaticMarkup(<BazarrSettingsPanel />);
    expect(html).toContain("Bazarr address");
    expect(html).toContain("API key");
    expect(html).toContain("Save");
    // nothing to remove or test until it is configured
    expect(html).not.toContain("Remove");
  });
});
