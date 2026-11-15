import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { BrandMark } from "./BrandMark.jsx";

describe("BrandMark", () => {
  it("shows the app icon, decorative, at the given size", () => {
    const html = renderToStaticMarkup(<BrandMark size={28} />);
    expect(html).toContain('src="/icon-192.png"');
    expect(html).toContain('width="28"');
    expect(html).toContain('aria-hidden="true"');
  });

  it("follows the URL base the page was served under", () => {
    globalThis.window = { __MEMEARR_BASE__: "/meme-arr" };
    try {
      expect(renderToStaticMarkup(<BrandMark />)).toContain('src="/meme-arr/icon-192.png"');
    } finally {
      delete globalThis.window;
    }
  });
});
