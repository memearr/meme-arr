// How big the caption drawn over the editing preview should be, so it matches
// the export from the first frame (no "check with burned-in captions" needed).
//
// Burned subtitles (libass, an SRT): sizes are in a 288-line space, whatever the
// video's own height, so text is a fixed fraction of the picture's height.
// FontSize is the line cell (~1.15 em), the CSS font-size is the em.
// Meme text (drawtext): sizes are pixels of the output frame, which is `outWidth`
// wide, so text is a fixed fraction of the picture's width.

const SRT_RES_Y = 288;
const SRT_DEFAULT_SIZE = 16;
const SRT_MARGIN_V = 10;
const SRT_OUTLINE = 2;
const LINE_TO_EM = 1.15;

/** The picture's rectangle inside an element that letterboxes it (object-fit: contain). */
export function containedRect(boxW, boxH, naturalW, naturalH) {
  if (!(boxW > 0 && boxH > 0 && naturalW > 0 && naturalH > 0)) return null;
  const scale = Math.min(boxW / naturalW, boxH / naturalH);
  const w = naturalW * scale;
  const h = naturalH * scale;
  return { left: (boxW - w) / 2, top: (boxH - h) / 2, width: w, height: h };
}

/**
 * Pixel sizes for the caption overlay on a picture `width` x `height` on screen.
 * `mode` is "source" or "meme"; `scalePct` the text size slider; `memeSize` the
 * meme text's base size in output pixels; `outWidth` the export width.
 */
export function captionMetrics({ mode, width, height, scalePct = 100, memeSize = 36, outWidth = 480 }) {
  if (!(width > 0 && height > 0)) return null;
  const pct = Math.max(1, scalePct) / 100;
  if (mode === "meme") {
    const k = width / Math.max(1, outWidth);
    return {
      fontSize: Math.max(6, memeSize * pct) * k,
      outline: Math.max(1, 3 * k),
      margin: Math.max(8 * k, height * 0.05),
    };
  }
  const k = height / SRT_RES_Y;
  return {
    fontSize: (Math.max(6, SRT_DEFAULT_SIZE * pct) * k) / LINE_TO_EM,
    outline: SRT_OUTLINE * k,
    margin: SRT_MARGIN_V * k,
  };
}
