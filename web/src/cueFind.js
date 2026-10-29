// Browser-style find over the subtitle browser's line list: nothing is ever
// hidden; a query just yields the indices of matching lines to scroll to.

/** Indices into `cues` whose text contains `query` (trimmed, case-insensitive). */
export function findCueMatches(cues, query) {
  const q = query.trim().toLowerCase();
  if (!q) return [];
  const out = [];
  cues.forEach((c, i) => {
    if (c.text.toLowerCase().includes(q)) out.push(i);
  });
  return out;
}

/**
 * Move `delta` (±1) through `count` matches from position `current`, wrapping
 * at both ends. A stale or unset `current` (e.g. -1 before the reset effect
 * runs) starts from the first match going forward, the last going back.
 * Returns -1 when there are no matches.
 */
export function stepMatch(current, delta, count) {
  if (count === 0) return -1;
  if (current < 0 || current >= count) return delta < 0 ? count - 1 : 0;
  return (current + delta + count) % count;
}
