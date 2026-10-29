// "Take me to Wanted for this": what the Wanted tab should show when it is
// opened from a "no subtitles" / "N missing" notation elsewhere.
//
// A focus is `{ query, libraries, openKey, note }`:
//   query      text for the Wanted filter box (matches title or show)
//   libraries  library names to select in the picker (empty = all)
//   openKey    a rating key whose "Find subtitles" row should be opened, ready to search
//   single     open the only row if exactly one matches (used for a show)

/** One item (a movie or an episode) with no subtitles: open its search. */
export function focusForItem(item) {
  return {
    query: item.title || "",
    libraries: item.library ? [item.library] : [],
    openKey: item.rating_key,
    label: item.show ? `${item.show} · ${item.title}` : item.title,
  };
}

/** A show's missing episodes: filter by the show; if only one is missing, open it. */
export function focusForShow(name) {
  return { query: name, libraries: [], openKey: null, single: true, label: name };
}

/** Everything missing in one library. */
export function focusForLibrary(name) {
  return { query: "", libraries: [name], openKey: null, label: name };
}

/**
 * What to do once Wanted has loaded the list a focus asked for: the key to
 * open, or a note when the item isn't there (already has subtitles, or its file
 * can't be reached, so it never appears in Wanted).
 *
 * @returns {{ open: string|null, note: string|null }}
 */
export function resolveFocus(focus, items) {
  if (!focus) return { open: null, note: null };
  if (focus.openKey) {
    if (items.some((i) => i.rating_key === focus.openKey)) return { open: focus.openKey, note: null };
    return {
      open: null,
      note: `“${focus.label}” isn't on the Wanted list. It may already have subtitles, or its file can't be reached (check Settings → library path mappings).`,
    };
  }
  if (focus.single && items.length === 1) return { open: items[0].rating_key, note: null };
  return { open: null, note: null };
}
