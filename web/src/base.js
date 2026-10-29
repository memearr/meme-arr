// The sub-path the app is hosted under (the server's URL_BASE, e.g.
// "/meme-arr"), injected into index.html as window.__MEMEARR_BASE__. "" = root.

export function getBase(win = globalThis.window) {
  return win?.__MEMEARR_BASE__ || "";
}

/**
 * Prefix a root-relative path ("/api/...") with the base. Anything else
 * (absolute URLs, blob: URLs, already-prefixed paths) passes through, so it's
 * safe to apply to a URL more than once.
 */
export function withBase(path, base = getBase()) {
  if (!base || !path.startsWith("/") || path.startsWith("//")) return path;
  if (path === base || path.startsWith(`${base}/`)) return path;
  return base + path;
}
