import { useEffect, useRef, useState } from "react";
import { api } from "./api.js";
import { fpsValue } from "./timecode.js";

/** The editor never grows a clip past what the server will preview in one go. */
const PREVIEW_MAX_MS = 60_000;
/** Footage rendered either side of the clip, so trimming wider needs no new render. */
const PROXY_PAD_MS = 3000;
/** The picture-only working copy is drawn this wide: small, so it renders fast. */
const PROXY_WIDTH = 480;
/**
 * The working preview: one picture-only render of the clip plus some footage
 * either side. Moving the In/Out points inside it needs no new render: the
 * player just seeks. A new render is only made when the clip leaves what was
 * rendered, a moment after the trim stops moving.
 *
 * Returns the render (`proxy`: `{ src, from, to }` in media time), its state,
 * and the refs the player needs (`videoRef` for the `<video>`, `proxyRef`
 * always holding the current render, `previewStarted` set once it plays).
 */
export function usePreviewProxy({ ratingKey, width, srcFps, ready, start, end, mediaDuration, tooLong }) {
  const [proxy, setProxy] = useState(null); // { src, from, to }
  const [proxyLoading, setProxyLoading] = useState(false);
  const [previewError, setPreviewError] = useState(null);
  const [previewStalled, setPreviewStalled] = useState(false);
  const previewUrlRef = useRef(null);
  const previewSeq = useRef(0);
  const previewAbort = useRef(null);
  const previewStarted = useRef(false);
  const videoRef = useRef(null);
  const proxyRef = useRef(null);
  proxyRef.current = proxy;

  async function renderProxy(wStart, wEnd) {
    // A newer request replaces an older one: cancel it (the server stops
    // rendering it) and never let its answer arrive after the newer one's.
    previewAbort.current?.abort();
    const ctrl = new AbortController();
    previewAbort.current = ctrl;
    const seq = ++previewSeq.current;
    setProxyLoading(true);
    setPreviewError(null);
    setPreviewStalled(false);
    previewStarted.current = false;
    try {
      const url = await api.fetchBlobUrl(
        api.previewUrl(ratingKey, {
          start: wStart,
          end: wEnd,
          width: Math.min(width, PROXY_WIDTH),
          fps: srcFps ? Math.min(30, Math.max(8, Math.round(fpsValue(srcFps)))) : 24,
          captionMode: "none",
        }),
        ctrl.signal
      );
      if (seq !== previewSeq.current) {
        URL.revokeObjectURL(url); // superseded while it was loading
        return;
      }
      if (previewUrlRef.current) URL.revokeObjectURL(previewUrlRef.current);
      previewUrlRef.current = url;
      setProxy({ src: url, from: wStart, to: wEnd });
    } catch (e) {
      if (e.name === "AbortError" || seq !== previewSeq.current) return;
      setProxy(null);
      setPreviewError(e.message);
    } finally {
      if (seq === previewSeq.current) setProxyLoading(false);
    }
  }

  // Keep a render that covers the clip. Anything inside the current one is free;
  // otherwise render again a moment after the trim stops moving.
  useEffect(() => {
    if (!ready || tooLong || end <= start) return undefined;
    if (proxy && start >= proxy.from && end <= proxy.to) return undefined;
    const pad = Math.max(0, Math.min(PROXY_PAD_MS, Math.floor((PREVIEW_MAX_MS - (end - start)) / 2)));
    const wStart = Math.max(0, start - pad);
    let wEnd = end + pad;
    if (mediaDuration > 0) wEnd = Math.min(mediaDuration, wEnd);
    const t = setTimeout(() => renderProxy(wStart, wEnd), 350);
    return () => clearTimeout(t);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ready, start, end, proxy, mediaDuration, tooLong]);

  // A render that is on screen but never starts (the thing that looks like a
  // button that does nothing): say so after a few seconds instead of leaving it.
  useEffect(() => {
    if (!proxy) return undefined;
    const t = setTimeout(() => {
      if (!previewStarted.current) setPreviewStalled(true);
    }, 6000);
    return () => clearTimeout(t);
  }, [proxy]);

  // Stop any render still under way, and free the video, when the editor closes.
  useEffect(
    () => () => {
      previewAbort.current?.abort();
      if (previewUrlRef.current) URL.revokeObjectURL(previewUrlRef.current);
    },
    []
  );

  return {
    proxy,
    proxyLoading,
    previewError,
    setPreviewError,
    previewStalled,
    setPreviewStalled,
    previewStarted,
    videoRef,
    proxyRef,
  };
}
