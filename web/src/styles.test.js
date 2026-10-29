import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

const css = readFileSync(new URL("./styles.css", import.meta.url), "utf8");

/** The `--name: value` declarations of the block that starts with `marker`. */
function palette(marker) {
  const start = css.indexOf(marker);
  expect(start, `missing block ${marker}`).toBeGreaterThan(-1);
  const body = css.slice(start, css.indexOf("}", start));
  return Object.fromEntries([...body.matchAll(/--([\w-]+):\s*([^;]+);/g)].map((m) => [m[1], m[2].trim()]));
}

const dark = palette(":root {");
const light = palette(':root[data-theme="light"] {');
const autoLight = palette(':root:not([data-theme="dark"]):not([data-theme="light"]) {');

function luminance(hex) {
  const h = hex.replace("#", "");
  const [r, g, b] = [0, 2, 4].map((i) => parseInt(h.slice(i, i + 2), 16) / 255);
  const f = (c) => (c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4);
  return 0.2126 * f(r) + 0.7152 * f(g) + 0.0722 * f(b);
}
function contrast(a, b) {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
}

describe("theme palettes", () => {
  // The light palette has to be written twice (explicit + Auto media query).
  it("Auto-light is identical to explicit light", () => {
    expect(autoLight).toEqual(light);
  });

  it("light defines every variable dark does", () => {
    const missing = Object.keys(dark).filter((k) => !(k in light) && k !== "sidebar-w" && k !== "topbar-h");
    expect(missing).toEqual([]);
  });

  // [foreground, background]: text pairs used by the stylesheet.
  const TEXT_PAIRS = [
    ["text", "bg"],
    ["text", "panel"],
    ["text", "panel-2"],
    ["muted", "bg"],
    ["muted", "panel"],
    ["muted", "panel-2"],
    ["error", "panel"],
    ["good", "panel"],
    ["warn", "panel"],
    ["warn", "panel-2"],
    ["danger-fg", "danger-bg"],
    ["notice-fg", "notice-bg"],
    ["on-error", "error"],
    ["on-warn", "warn"],
  ];

  // Light is new and held to AA. Dark's violet accent as text on panels
  // (3.97) and white-on-accent buttons (4.23) predate this and are excluded.
  it.each(TEXT_PAIRS)("light: %s on %s meets WCAG AA (4.5:1)", (fg, bg) => {
    expect(contrast(light[fg], light[bg])).toBeGreaterThanOrEqual(4.5);
  });

  it.each(TEXT_PAIRS)("dark: %s on %s meets WCAG AA (4.5:1)", (fg, bg) => {
    expect(contrast(dark[fg], dark[bg])).toBeGreaterThanOrEqual(4.5);
  });

  it("light: accent text and white-on-accent buttons meet AA", () => {
    expect(contrast(light.accent, light.panel)).toBeGreaterThanOrEqual(4.5);
    expect(contrast("#ffffff", light.accent)).toBeGreaterThanOrEqual(4.5);
  });
});
