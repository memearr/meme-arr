import { describe, expect, it } from "vitest";
import { EMPTY_HISTORY, HISTORY_LIMIT, recordTrim, stepBack, stepForward } from "./useTrim.js";

const at = (start, end) => ({ start, end });

describe("trim history", () => {
  it("records a finished change as one step and clears what could be redone", () => {
    const h = recordTrim({ past: [], future: [at(9, 99)] }, at(0, 5000), at(1000, 4000));
    expect(h.past).toEqual([at(0, 5000)]);
    expect(h.future).toEqual([]);
  });

  it("ignores a change that ends where it began", () => {
    expect(recordTrim(EMPTY_HISTORY, at(1, 2), at(1, 2))).toBe(EMPTY_HISTORY);
  });

  it("keeps only the most recent steps", () => {
    let h = EMPTY_HISTORY;
    for (let i = 0; i < HISTORY_LIMIT + 20; i++) h = recordTrim(h, at(i, i + 10), at(i + 1, i + 10));
    expect(h.past).toHaveLength(HISTORY_LIMIT);
    expect(h.past[h.past.length - 1]).toEqual(at(HISTORY_LIMIT + 19, HISTORY_LIMIT + 29));
  });

  it("steps back to the previous trim and keeps the present one to redo", () => {
    const h = recordTrim(EMPTY_HISTORY, at(0, 5000), at(1000, 4000));
    const r = stepBack(h, at(1000, 4000));
    expect(r.to).toEqual(at(0, 5000));
    expect(r.history.past).toEqual([]);
    expect(r.history.future).toEqual([at(1000, 4000)]);
  });

  it("steps forward again", () => {
    const back = stepBack(recordTrim(EMPTY_HISTORY, at(0, 5000), at(1000, 4000)), at(1000, 4000));
    const fwd = stepForward(back.history, at(0, 5000));
    expect(fwd.to).toEqual(at(1000, 4000));
    expect(fwd.history.past).toEqual([at(0, 5000)]);
    expect(fwd.history.future).toEqual([]);
  });

  it("has nothing to undo or redo on a fresh history", () => {
    expect(stepBack(EMPTY_HISTORY, at(0, 1))).toBeNull();
    expect(stepForward(EMPTY_HISTORY, at(0, 1))).toBeNull();
  });
});
