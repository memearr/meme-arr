// Helpers for the System -> Logs panel.

const CAP = 2000;

/** One entry as a plain-text line: `HH:MM:SS.mmm LEVEL target message`. */
export function formatLogLine(e) {
  const d = new Date(e.ts_ms);
  const pad = (n, w = 2) => String(n).padStart(w, "0");
  const time = `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}.${pad(d.getMilliseconds(), 3)}`;
  return `${time} ${e.level.toUpperCase().padEnd(5)} ${e.target} ${e.message}`;
}

/**
 * Append newly fetched entries to those already shown, ignoring any already
 * present (by id) and keeping at most `cap` of the newest.
 */
export function mergeEntries(prev, incoming, cap = CAP) {
  if (incoming.length === 0) return prev;
  const have = new Set(prev.map((e) => e.id));
  const merged = [...prev, ...incoming.filter((e) => !have.has(e.id))];
  return merged.length > cap ? merged.slice(merged.length - cap) : merged;
}

/** The highest entry id seen, or null -- what to pass as `after` when polling. */
export function lastId(entries) {
  return entries.length ? entries[entries.length - 1].id : null;
}

/** The plain-text body of a log download. */
export function entriesToText(entries) {
  return entries.map(formatLogLine).join("\n") + (entries.length ? "\n" : "");
}
