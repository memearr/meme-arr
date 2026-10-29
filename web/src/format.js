// Pure display formatters, kept out of App.jsx so they can be unit-tested.

/** Milliseconds as `m:ss.mmm` (e.g. 83456 -> "1:23.456"); "–" for null. */
export function fmtTime(ms) {
  if (ms == null) return "–";
  const sign = ms < 0 ? "-" : "";
  ms = Math.abs(ms);
  const m = Math.floor(ms / 60000);
  const s = Math.floor((ms % 60000) / 1000);
  const frac = Math.floor(ms % 1000);
  return `${sign}${m}:${String(s).padStart(2, "0")}.${String(frac).padStart(3, "0")}`;
}

/** "Show · S01E02" for episodes, "Title (Year)" for movies. */
export function episodeLabel(r) {
  if (r.show) {
    const se =
      r.season != null && r.episode != null
        ? ` · S${String(r.season).padStart(2, "0")}E${String(r.episode).padStart(2, "0")}`
        : "";
    return `${r.show}${se}`;
  }
  return r.year ? `${r.title} (${r.year})` : r.title;
}

/** A task interval: "1 hour", "12 hours", "1 day", "7 days". */
export function fmtInterval(secs) {
  const h = Math.round(secs / 3600);
  if (h < 24 || h % 24 !== 0) return `${h} hour${h === 1 ? "" : "s"}`;
  const d = h / 24;
  return `${d} day${d === 1 ? "" : "s"}`;
}

/**
 * A unix-seconds timestamp relative to `now` (also unix seconds):
 * "just now", "5m ago", "3h ago", "2d ago", or "in 5m" / "in 3h" / "in 2d".
 * "–" when there's no timestamp.
 */
export function fmtRelative(ts, now) {
  if (ts == null) return "–";
  const diff = ts - now;
  const abs = Math.abs(diff);
  if (abs < 60) return diff > 0 ? "in <1m" : "just now";
  const [n, unit] =
    abs < 3600 ? [Math.floor(abs / 60), "m"] : abs < 86400 ? [Math.floor(abs / 3600), "h"] : [Math.floor(abs / 86400), "d"];
  return diff > 0 ? `in ${n}${unit}` : `${n}${unit} ago`;
}

/** A run duration: "850ms", "4.2s", "3m 05s"; "–" when unknown. */
export function fmtDuration(ms) {
  if (ms == null) return "–";
  if (ms < 1000) return `${ms}ms`;
  if (ms < 60_000) return `${(ms / 1000).toFixed(1)}s`;
  const m = Math.floor(ms / 60_000);
  const s = Math.floor((ms % 60_000) / 1000);
  return `${m}m ${String(s).padStart(2, "0")}s`;
}

/** Human-readable byte size in binary units (KB = 1024 B). */
export function fmtBytes(n) {
  if (n == null) return "–";
  if (n < 1024) return `${n} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let v = n / 1024;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v.toFixed(v < 10 ? 2 : 1)} ${units[i]}`;
}
