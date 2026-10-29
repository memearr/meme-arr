// Light / dark / auto theme. "auto" means no data-theme attribute, so the
// stylesheet's prefers-color-scheme media query decides.

export const THEME_KEY = "memearr:theme";
export const THEMES = ["auto", "light", "dark"];

/** A stored value if it's a known theme, else "auto". */
export function parseTheme(stored) {
  return THEMES.includes(stored) ? stored : "auto";
}

/** The next theme when cycling Auto -> Light -> Dark -> Auto. */
export function nextTheme(current) {
  return THEMES[(THEMES.indexOf(parseTheme(current)) + 1) % THEMES.length];
}

/** Set (or, for "auto", remove) data-theme on <html> and remember the choice. */
export function applyTheme(theme, root = document.documentElement, storage = globalThis.localStorage) {
  const t = parseTheme(theme);
  if (t === "auto") root.removeAttribute("data-theme");
  else root.setAttribute("data-theme", t);
  try {
    storage?.setItem(THEME_KEY, t);
  } catch (_) {
    /* private mode / blocked storage: applies for this session only */
  }
  return t;
}

export function storedTheme(storage = globalThis.localStorage) {
  try {
    return parseTheme(storage?.getItem(THEME_KEY));
  } catch (_) {
    return "auto";
  }
}
