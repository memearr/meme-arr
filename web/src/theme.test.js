import { describe, expect, it } from "vitest";
import { applyTheme, nextTheme, parseTheme, storedTheme } from "./theme.js";

function fakeRoot() {
  const attrs = {};
  return {
    attrs,
    setAttribute: (k, v) => (attrs[k] = v),
    removeAttribute: (k) => delete attrs[k],
  };
}
const memStorage = (init = {}) => ({
  data: { ...init },
  getItem(k) {
    return k in this.data ? this.data[k] : null;
  },
  setItem(k, v) {
    this.data[k] = v;
  },
});

describe("theme", () => {
  it("falls back to auto for anything unknown", () => {
    expect(parseTheme("light")).toBe("light");
    expect(parseTheme("purple")).toBe("auto");
    expect(parseTheme(null)).toBe("auto");
  });

  it("cycles auto -> light -> dark -> auto", () => {
    expect(nextTheme("auto")).toBe("light");
    expect(nextTheme("light")).toBe("dark");
    expect(nextTheme("dark")).toBe("auto");
    expect(nextTheme("garbage")).toBe("light");
  });

  it("sets data-theme for light/dark and removes it for auto, persisting the choice", () => {
    const root = fakeRoot();
    const storage = memStorage();
    applyTheme("dark", root, storage);
    expect(root.attrs["data-theme"]).toBe("dark");
    expect(storage.data["memearr:theme"]).toBe("dark");
    applyTheme("auto", root, storage);
    expect("data-theme" in root.attrs).toBe(false);
    expect(storage.data["memearr:theme"]).toBe("auto");
  });

  it("survives unavailable storage", () => {
    const broken = {
      getItem() {
        throw new Error("blocked");
      },
      setItem() {
        throw new Error("blocked");
      },
    };
    expect(storedTheme(broken)).toBe("auto");
    expect(applyTheme("light", fakeRoot(), broken)).toBe("light");
  });

  it("reads back what was stored", () => {
    expect(storedTheme(memStorage({ "memearr:theme": "light" }))).toBe("light");
  });
});
