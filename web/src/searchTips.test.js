import { describe, expect, it } from "vitest";
import { SEARCH_TIPS, SEARCH_TIPS_FOOTER } from "./searchTips.js";

describe("search tips", () => {
  it("document every operator the server understands", () => {
    const text = SEARCH_TIPS.map((t) => t.example).join(" ");
    for (const op of ["show:", "ep:", "movie:", "library:", "season:"]) {
      expect(text).toContain(op);
    }
    expect(text).toContain("-show:"); // exclusion
    expect(text).toMatch(/-walter/); // excluding a word
  });
  it("have a distinct example and an explanation for each tip", () => {
    const examples = SEARCH_TIPS.map((t) => t.example);
    expect(new Set(examples).size).toBe(examples.length);
    for (const t of SEARCH_TIPS) expect(t.says.length).toBeGreaterThan(5);
  });
  it("tell people how to search operator-looking text literally", () => {
    expect(SEARCH_TIPS_FOOTER).toMatch(/quotes/);
  });
});
