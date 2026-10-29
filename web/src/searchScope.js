// The filters applied to a search, kept apart from the text being searched, so
// you can narrow to a show (or a season, or only movies...) first and then search
// the dialogue *inside* that subset. They are sent to the server as the same
// operators the search box understands (src/querysyntax.rs): `show:"X"`,
// `-ep:pilot`, `season:2`, `movie:`; so a filter chip and a typed operator are
// the same thing, and typed ones still work.
//
// A filter is { field, value, negate } with field one of
// "show" | "episode" | "movie" | "season"; an empty value on episode/movie
// means "only episodes" / "only movies".

export const FILTER_FIELDS = [
  { id: "show", label: "Show contains", placeholder: "e.g. Orbital Diner" },
  { id: "episode", label: "Episode title contains", placeholder: "e.g. docking" },
  { id: "movie", label: "Movie title contains", placeholder: "e.g. lighthouse" },
  { id: "season", label: "Season number", placeholder: "e.g. 2" },
  { id: "only-episodes", label: "Only episodes", placeholder: "", bare: true },
  { id: "only-movies", label: "Only movies", placeholder: "", bare: true },
];

const KEY = { show: "show", episode: "ep", movie: "movie", season: "season" };

/** A validated filter from what the "Add filter" form collected, or null if it isn't usable. */
export function makeFilter(fieldId, value, negate = false) {
  const v = String(value ?? "").replace(/"/g, "").trim();
  switch (fieldId) {
    case "show":
    case "episode":
    case "movie":
      return v ? { field: fieldId, value: v, negate: !!negate } : null;
    case "season": {
      if (!/^\d+$/.test(v)) return null;
      return { field: "season", value: String(Number(v)), negate: !!negate };
    }
    case "only-episodes":
      return { field: "episode", value: "", negate: !!negate };
    case "only-movies":
      return { field: "movie", value: "", negate: !!negate };
    default:
      return null;
  }
}

/** The operator text for one filter: show:"Lantern Cove", -ep:docking, movie: */
export function filterToken(f) {
  const v = f.value || "";
  const value = /\s/.test(v) ? `"${v}"` : v;
  return `${f.negate ? "-" : ""}${KEY[f.field]}:${value}`;
}

/** What goes to the search API: the text, then each filter as an operator. */
export function buildQuery(text, scope) {
  return [String(text || "").trim(), ...(scope || []).map(filterToken)].filter(Boolean).join(" ");
}

/** How the page shows a filter back; the same wording the server uses for what it understood. */
export function describeFilter(f) {
  if (!f.value) {
    const only = f.field === "episode" ? "episodes" : "movies";
    return f.negate ? `not ${only}` : `${only} only`;
  }
  const name = { show: "show", episode: "episode", movie: "movie", season: "season" }[f.field];
  return `${f.negate ? "not " : ""}${name}: ${f.value}`;
}

export function sameFilter(a, b) {
  return a.field === b.field && a.value.toLowerCase() === b.value.toLowerCase() && !!a.negate === !!b.negate;
}

/** Add a filter unless the same one is already there. */
export function addFilter(scope, f) {
  if (!f || scope.some((x) => sameFilter(x, f))) return scope;
  return [...scope, f];
}

export function removeFilterAt(scope, i) {
  return scope.filter((_, j) => j !== i);
}

/**
 * The server's description of how it read the query, minus the ones that came
 * from filter chips (which are shown as chips already): what was understood
 * from operators typed into the box.
 */
export function typedOnly(understood, scope) {
  const rest = [...(understood || [])];
  for (const f of scope || []) {
    const i = rest.indexOf(describeFilter(f));
    if (i >= 0) rest.splice(i, 1);
  }
  return rest;
}
