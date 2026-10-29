// Pure helpers for caption fonts and text size (the font picker in the editor and the
// font list in Settings). The server does the real work; this keeps the choices tidy.

/** Text size as a percentage of normal. The server accepts 25 to 400; the slider offers a useful range. */
export const SCALE_MIN = 50;
export const SCALE_MAX = 300;
export const SCALE_STEP = 10;
export const SCALE_DEFAULT = 100;

const STORE_KEY = "memearr:caption-font";

/** A whole-number percentage within the slider's range; anything unusable becomes 100. */
export function normalizeScale(value) {
  const n = Math.round(Number(value));
  if (!Number.isFinite(n)) return SCALE_DEFAULT;
  return Math.min(SCALE_MAX, Math.max(SCALE_MIN, n));
}

/** "Anton", or "Roboto Bold" when the style says more than "Regular". */
export function fontName(font) {
  const style = (font.style || "").trim();
  return style && style.toLowerCase() !== "regular" ? `${font.family} ${style}` : font.family;
}

/**
 * The picker's entries: the default first, then built-in fonts, then uploaded ones.
 * `list` is `GET /api/fonts`'s `fonts`; `fallback` its `default`.
 */
export function fontOptions(list, fallback) {
  const options = [{ id: "", label: fallback ? `Default (${fontName(fallback)})` : "Default" }];
  for (const f of list || []) {
    options.push({ id: f.id, label: f.builtin ? `${fontName(f)} (built in)` : fontName(f) });
  }
  return options;
}

/** Why `file` can't be uploaded as a font, or null if it can. Mirrors the server's checks. */
export function uploadProblem(file, maxBytes) {
  if (!file) return "Choose a font file.";
  if (!/\.(ttf|otf)$/i.test(file.name)) return "Only .ttf and .otf font files are supported.";
  if (maxBytes && file.size > maxBytes) {
    return `That file is larger than ${Math.round(maxBytes / 1024 / 1024)} MB.`;
  }
  if (!/^[A-Za-z0-9._ ()-]+$/.test(file.name)) {
    return "The file name may only contain letters, digits, spaces and . _ - ( ).";
  }
  return null;
}

/** "128 KB" / "1.2 MB". */
export function fileSize(bytes) {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

/** The last font and size used, so the editor starts where you left off. */
export function loadFontChoice(storage) {
  try {
    const v = JSON.parse(storage?.getItem(STORE_KEY) || "null");
    return {
      font: typeof v?.font === "string" ? v.font : "",
      scale: normalizeScale(v?.scale ?? SCALE_DEFAULT),
    };
  } catch (_) {
    return { font: "", scale: SCALE_DEFAULT };
  }
}

export function saveFontChoice(storage, font, scale) {
  try {
    storage?.setItem(STORE_KEY, JSON.stringify({ font: font || "", scale: normalizeScale(scale) }));
  } catch (_) {
    /* storage unavailable: the choice just isn't remembered */
  }
}

/** A remembered font that has since been deleted falls back to the default. */
export function usableFont(id, list) {
  return id && (list || []).some((f) => f.id === id) ? id : "";
}
