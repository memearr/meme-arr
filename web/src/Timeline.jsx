import { useEffect, useRef, useState } from "react";
import { api } from "./api.js";
import { fmtTime } from "./format.js";
import {
  MIN_CLIP_MS,
  fromPixels,
  msToFrame,
  snapToEdges,
  snapToFrame,
  stepFrames,
  toPercent,
} from "./timecode.js";

const STRIP_FRAMES = 12;
const MAX_STRIP_SPAN_MS = 120_000; // the longest span the server draws a filmstrip for
const TICK_STEPS = [100, 200, 500, 1000, 2000, 5000, 10_000, 15_000, 30_000, 60_000, 300_000];

/** A tick spacing that keeps labels about 80px apart. */
export function tickStep(spanMs, widthPx) {
  const want = (spanMs / Math.max(widthPx, 1)) * 80;
  return TICK_STEPS.find((s) => s >= want) ?? TICK_STEPS[TICK_STEPS.length - 1];
}

function pct(ms, view) {
  return `${toPercent(ms, view.from, view.to)}%`;
}

/**
 * The editing timeline: a filmstrip of the part of the film in view, the clip's
 * In/Out handles, the playhead and the subtitle lines. Everything is dragged;
 * the handles also answer the arrow keys. Times are reported through
 * `onTrim` / `onCueMove` / `onSeek`; `commit` is false while a drag is under
 * way and true when it ends, so the editor can keep one undo step per drag.
 */
export function Timeline({
  ratingKey,
  view,
  start,
  end,
  playhead,
  fps,
  snap, // "frame" | "caption" | "off"
  cues, // [{ start_ms, end_ms, text, index: number|null }]; index set = editable
  onTrim,
  onCueMove,
  onSeek,
  onZoom,
  onFit,
}) {
  const trackRef = useRef(null);
  const drag = useRef(null);
  const [width, setWidth] = useState(800);
  const [stripUrl, setStripUrl] = useState(null);
  const [stripFailed, setStripFailed] = useState(false);

  useEffect(() => {
    const el = trackRef.current;
    if (!el) return undefined;
    setWidth(el.clientWidth || 800);
    if (typeof ResizeObserver === "undefined") return undefined;
    const ro = new ResizeObserver(() => setWidth(el.clientWidth || 800));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  // The filmstrip follows the view, a moment after it stops moving.
  useEffect(() => {
    const span = view.to - view.from;
    if (span > MAX_STRIP_SPAN_MS) {
      setStripUrl(null);
      return undefined;
    }
    const t = setTimeout(() => {
      setStripFailed(false);
      setStripUrl(api.filmstripUrl(ratingKey, view.from, view.to, STRIP_FRAMES));
    }, 300);
    return () => clearTimeout(t);
  }, [ratingKey, view.from, view.to]);

  const span = view.to - view.from;
  const msPerPx = span / Math.max(width, 1);
  const edges = cues.flatMap((c) => [c.start_ms, c.end_ms]);

  function snapped(ms) {
    if (snap === "frame" && fps) return snapToFrame(ms, fps);
    if (snap === "caption") return snapToEdges(ms, edges, msPerPx * 8);
    return Math.round(ms);
  }

  function timeAt(e) {
    const rect = trackRef.current.getBoundingClientRect();
    return fromPixels(e.clientX - rect.left, rect.width, view.from, view.to);
  }

  function begin(kind, e, extra = {}) {
    e.stopPropagation();
    e.currentTarget.setPointerCapture?.(e.pointerId);
    drag.current = { kind, at: timeAt(e), start, end, ...extra };
  }

  function move(e) {
    const d = drag.current;
    if (!d) return;
    const now = timeAt(e);
    if (d.kind === "in") {
      onTrim(Math.min(snapped(now), end - MIN_CLIP_MS), end, false, "start");
    } else if (d.kind === "out") {
      onTrim(start, Math.max(snapped(now), start + MIN_CLIP_MS), false, "end");
    } else if (d.kind === "band") {
      const s = snapped(d.start + (now - d.at));
      onTrim(s, s + (d.end - d.start), false, "start");
    } else if (d.kind === "cue") {
      const s = snapped(d.start + (now - d.at));
      onCueMove(d.index, s, s + (d.end - d.start), false);
    } else if (d.kind === "seek") {
      onSeek(Math.max(0, snapped(now)));
    }
  }

  function finish(e) {
    const d = drag.current;
    drag.current = null;
    if (!d) return;
    e.currentTarget.releasePointerCapture?.(e.pointerId);
    if (d.kind === "in" || d.kind === "out" || d.kind === "band") onTrim(start, end, true, "end");
    if (d.kind === "cue") onCueMove(d.index, null, null, true); // null: keep where it is, just end the drag
  }

  // Handles also answer the keyboard: arrows step a frame (Shift: ten).
  function handleKey(which, e) {
    const dir = e.key === "ArrowLeft" ? -1 : e.key === "ArrowRight" ? 1 : 0;
    if (!dir) return;
    e.preventDefault();
    const n = dir * (e.shiftKey ? 10 : 1);
    const step = (ms) => (fps ? stepFrames(ms, n, fps) : Math.max(0, ms + n * 40));
    if (which === "in") onTrim(Math.min(step(start), end - MIN_CLIP_MS), end, true, "start");
    else onTrim(start, Math.max(step(end), start + MIN_CLIP_MS), true, "end");
  }

  const step = tickStep(span, width);
  const ticks = [];
  for (let t = Math.ceil(view.from / step) * step; t <= view.to; t += step) ticks.push(t);

  const inView = (c) => c.end_ms > view.from && c.start_ms < view.to;

  return (
    <div className="timeline">
      <div className="timeline-bar">
        <span className="tl-label">Timeline</span>
        <span className="tl-range">
          {fmtTime(view.from)} – {fmtTime(view.to)}
        </span>
        <span className="tl-spacer" />
        <button
          type="button"
          className="secondary"
          aria-label="Zoom out"
          onClick={() => onZoom(2, (start + end) / 2)}
        >
          −
        </button>
        <button
          type="button"
          className="secondary"
          aria-label="Zoom in"
          onClick={() => onZoom(0.5, (start + end) / 2)}
        >
          +
        </button>
        <button type="button" className="secondary" onClick={onFit}>
          Fit clip
        </button>
      </div>

      <div
        className="tl-track"
        ref={trackRef}
        onWheel={(e) => {
          if (!e.ctrlKey && !e.shiftKey) return;
          e.preventDefault();
          onZoom(e.deltaY > 0 ? 1.25 : 0.8, timeAt(e));
        }}
      >
        <div className="tl-flags">
          <span className="tl-flag in" style={{ right: `${100 - toPercent(start, view.from, view.to)}%` }}>
            IN {fmtTime(start)}
            {fps ? ` · f${msToFrame(start, fps)}` : ""}
          </span>
          <span className="tl-flag out" style={{ left: pct(end, view) }}>
            OUT {fmtTime(end)}
            {fps ? ` · f${msToFrame(end, fps)}` : ""}
          </span>
        </div>

        <div
          className="tl-ruler"
          onPointerDown={(e) => begin("seek", e)}
          onPointerMove={move}
          onPointerUp={finish}
          onPointerCancel={finish}
        >
          {ticks.map((t) => (
            <span key={t} className="tl-tick" style={{ left: pct(t, view) }}>
              {fmtTime(t).replace(/\.000$/, "")}
            </span>
          ))}
        </div>

        <div
          className="tl-strip"
          onPointerDown={(e) => begin("seek", e)}
          onPointerMove={move}
          onPointerUp={finish}
          onPointerCancel={finish}
        >
          {stripUrl && !stripFailed && (
            <img
              src={stripUrl}
              alt=""
              draggable={false}
              onError={() => setStripFailed(true)}
            />
          )}
          <div className="tl-dim" style={{ left: 0, width: pct(start, view) }} />
          <div className="tl-dim" style={{ left: pct(end, view), right: 0 }} />
          <div
            className="tl-band"
            style={{ left: pct(start, view), width: `${toPercent(end, view.from, view.to) - toPercent(start, view.from, view.to)}%` }}
            title="Drag to slide the whole clip"
            onPointerDown={(e) => begin("band", e)}
            onPointerMove={move}
            onPointerUp={finish}
            onPointerCancel={finish}
          />
          <button
            type="button"
            className="tl-handle in"
            style={{ left: pct(start, view) }}
            aria-label={`In point ${fmtTime(start)}`}
            onPointerDown={(e) => begin("in", e)}
            onPointerMove={move}
            onPointerUp={finish}
            onPointerCancel={finish}
            onKeyDown={(e) => handleKey("in", e)}
          />
          <button
            type="button"
            className="tl-handle out"
            style={{ left: pct(end, view) }}
            aria-label={`Out point ${fmtTime(end)}`}
            onPointerDown={(e) => begin("out", e)}
            onPointerMove={move}
            onPointerUp={finish}
            onPointerCancel={finish}
            onKeyDown={(e) => handleKey("out", e)}
          />
        </div>

        <div className="tl-captions">
          {cues.filter(inView).map((c, i) => {
            const editable = c.index != null;
            const inside = c.end_ms > start && c.start_ms < end;
            return (
              <div
                key={`${c.start_ms}-${i}`}
                className={"tl-cue" + (inside ? " inside" : "") + (editable ? " editable" : "")}
                style={{ left: pct(c.start_ms, view), width: `${toPercent(c.end_ms, view.from, view.to) - toPercent(c.start_ms, view.from, view.to)}%` }}
                title={`${c.text}\n${fmtTime(c.start_ms)} → ${fmtTime(c.end_ms)}`}
                onPointerDown={(e) =>
                  editable
                    ? begin("cue", e, { index: c.index, start: c.start_ms, end: c.end_ms })
                    : e.stopPropagation()
                }
                onPointerMove={move}
                onPointerUp={finish}
                onPointerCancel={finish}
                onDoubleClick={() => onTrim(c.start_ms, c.end_ms, true, "end")}
              >
                <span>{c.text}</span>
              </div>
            );
          })}
        </div>

        <div className="tl-playhead" style={{ left: pct(playhead, view) }} aria-hidden="true" />
      </div>

      <p className="hint tiny tl-help">
        Drag the handles to trim · drag the band to slide the clip · drag a subtitle block to retime it ·
        double-click a subtitle to trim to it · Ctrl/Shift + scroll to zoom
      </p>
    </div>
  );
}
