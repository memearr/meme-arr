import { describe, expect, it } from "vitest";
import { summarizeHealth } from "./health.js";

const issue = (level) => ({ kind: "x", level, message: "m", fix: "f", docs_url: "u" });

describe("summarizeHealth", () => {
  it("reports 'checking' until the first pass has finished", () => {
    expect(summarizeHealth(null).label).toBe("Checking…");
    expect(summarizeHealth({ checked_at: null, issues: [] }).count).toBe(0);
  });

  it("counts errors and warnings, with the worst level winning", () => {
    const s = summarizeHealth({ checked_at: 1, issues: [issue("warning"), issue("error"), issue("warning")] });
    expect(s).toEqual({ count: 3, level: "error", label: "1 error, 2 warnings" });
  });

  it("is a warning badge when there are only warnings", () => {
    expect(summarizeHealth({ checked_at: 1, issues: [issue("warning")] })).toEqual({
      count: 1,
      level: "warning",
      label: "1 warning",
    });
  });

  // "Update available" shouldn't light up a red/amber badge.
  it("doesn't count info-level issues as problems", () => {
    expect(summarizeHealth({ checked_at: 1, issues: [issue("info")] })).toEqual({
      count: 0,
      level: null,
      label: "No problems found",
    });
  });
});
