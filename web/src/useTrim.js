import { useRef, useState } from "react";
import { clampTrim } from "./timecode.js";

/** How many trims undo remembers. */
export const HISTORY_LIMIT = 100;

export const EMPTY_HISTORY = { past: [], future: [] };

/** The history after a finished change from `before` to `after` (unchanged if nothing moved). */
export function recordTrim(history, before, after) {
  if (before.start === after.start && before.end === after.end) return history;
  return { past: [...history.past, before].slice(-HISTORY_LIMIT), future: [] };
}

/** One step back from `current`: `{ history, to }`, or null when there is nothing to undo. */
export function stepBack(history, current) {
  if (history.past.length === 0) return null;
  const to = history.past[history.past.length - 1];
  return { to, history: { past: history.past.slice(0, -1), future: [current, ...history.future] } };
}

/** One step forward from `current`: `{ history, to }`, or null when there is nothing to redo. */
export function stepForward(history, current) {
  if (history.future.length === 0) return null;
  const to = history.future[0];
  return { to, history: { past: [...history.past, current], future: history.future.slice(1) } };
}

/**
 * The clip's In and Out points with undo and redo.
 *
 * `trim(s, e, commit, moved)` sets them: `commit` false is a change still under
 * way (a drag, a half-typed time); true ends it and makes it one undo step.
 * `moved` is the edge the caller changed, which decides which one gives way if
 * they collide. `setStart`/`setEnd` set a point without making an undo step.
 */
export function useTrim({ initialStart, initialEnd, mediaDuration }) {
  const [start, setStart] = useState(initialStart);
  const [end, setEnd] = useState(initialEnd);
  const [history, setHistory] = useState(EMPTY_HISTORY);
  const gesture = useRef(null); // the trim as it was before the change under way

  function trim(s, e, commit = true, moved = "end") {
    const t = clampTrim(Math.round(s), Math.round(e), mediaDuration || 0, moved);
    if (!gesture.current) gesture.current = { start, end };
    setStart(t.start);
    setEnd(t.end);
    if (commit) {
      const before = gesture.current;
      gesture.current = null;
      setHistory((h) => recordTrim(h, before, t));
    }
  }

  function goTo(step) {
    const r = step(history, { start, end });
    if (!r) return;
    setStart(r.to.start);
    setEnd(r.to.end);
    setHistory(r.history);
  }
  const undo = () => goTo(stepBack);
  const redo = () => goTo(stepForward);

  return { start, end, setStart, setEnd, history, trim, undo, redo };
}
