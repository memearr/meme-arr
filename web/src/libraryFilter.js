// Pure helpers for the library picker (which libraries a search or list covers).
// "Nothing selected" means every library, so a new library is never hidden by an
// old selection and the default needs no state.

/** Query-string pieces for the selected libraries: `&library=A&library=B` (empty = all). */
export function libraryParams(libraries) {
  return (libraries || [])
    .filter((l) => l && l.trim())
    .map((l) => `&library=${encodeURIComponent(l)}`)
    .join("");
}

/** Library list filter: [value sent as `status`, button label]. */
export const BROWSE_STATUSES = [
  ["all", "All"],
  ["missing", "Missing subtitles"],
  ["indexed", "Fully indexed"],
];

/** The picker's button label. */
export function libraryFilterLabel(selected, all) {
  const chosen = (selected || []).filter((l) => (all || []).includes(l));
  if (chosen.length === 0 || chosen.length === (all || []).length) return "All libraries";
  if (chosen.length === 1) return chosen[0];
  if (chosen.length === 2) return `${chosen[0]} + ${chosen[1]}`;
  return `${chosen.length} libraries`;
}

/** Tick or untick one library. Ticking every library collapses back to "all" (empty). */
export function toggleLibrary(selected, name, all) {
  const has = selected.includes(name);
  const next = has ? selected.filter((l) => l !== name) : [...selected, name];
  const known = next.filter((l) => (all || []).includes(l));
  return known.length === (all || []).length ? [] : next;
}

/** "Showing 50 matches (more available)" under a result list. */
export function resultCountText(count, hasMore) {
  if (count === 0) return "";
  const n = count.toLocaleString();
  if (hasMore) return `Showing the first ${n} matches`;
  return count === 1 ? "1 match" : `${n} matches`;
}
