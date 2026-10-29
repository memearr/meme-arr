import { describe, expect, it } from "vitest";
import { updateFailureHelp } from "./update.js";

describe("updateFailureHelp", () => {
  it("explains the private-repo 404 and what to do", () => {
    const h = updateFailureHelp("GitHub returned 404 Not Found");
    expect(h.reason).toBe("GitHub returned 404 Not Found");
    expect(h.fix).toMatch(/private repository/);
    expect(h.fix).toMatch(/GITHUB_TOKEN/);
    expect(h.fix).toMatch(/public/);
  });

  it("distinguishes a rejected token", () => {
    for (const e of ["GitHub returned 401 Unauthorized", "GitHub returned 403 Forbidden"]) {
      expect(updateFailureHelp(e).fix).toMatch(/rejected the token|no read access/);
    }
  });

  it("falls back to a network hint for anything else", () => {
    expect(updateFailureHelp("error sending request").fix).toMatch(/api\.github\.com/);
  });

  it("always has a reason, even with no error text", () => {
    expect(updateFailureHelp(undefined).reason).toBe("The update check failed.");
    expect(updateFailureHelp("   ").reason).toBe("The update check failed.");
  });

  it("separates 'our own server didn't answer' from GitHub problems", () => {
    const h = updateFailureHelp("GitHub returned 404", true);
    expect(h.reason).toMatch(/Couldn't reach the app/);
    expect(h.fix).not.toMatch(/GITHUB_TOKEN/);
  });

  it("doesn't mistake a number inside another number for a status", () => {
    expect(updateFailureHelp("failed after 14040 ms").fix).toMatch(/api\.github\.com/);
  });
});
