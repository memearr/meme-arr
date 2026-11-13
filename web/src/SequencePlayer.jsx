import { useEffect, useRef, useState } from "react";
import { api } from "./api.js";
import { episodeLabel, fmtTime } from "./format.js";
import { layout, locate, previewKey, previewParams, totalMs } from "./sequenceModel.js";

/**
 * Plays the whole sequence back to back: each section as its own preview
 * (captions burned in the way the export will draw them), the next one fetched
 * while the current plays. `index` is the section on screen and `onIndex`
 * changes it, so the timeline and the player stay together.
 */
export default function SequencePlayer({ sequence, width, fps, index, onIndex, onProgress }) {
  const [src, setSrc] = useState(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState(null);
  const [playing, setPlaying] = useState(false);
  const [muted, setMuted] = useState(false);
  const [elapsed, setElapsed] = useState(0);
  const cache = useRef(new Map()); // key -> object URL
  const inflight = useRef(new Map()); // key -> promise of an object URL
  const pendingOffset = useRef(0); // ms to jump to once the section loads
  const videoRef = useRef(null);

  const opts = { width, fps };
  const spans = layout(sequence);
  const total = totalMs(sequence);
  const clip = sequence[index];
  const clipKey = clip ? previewKey(clip, opts) : null;
  const allKeys = sequence.map((c) => previewKey(c, opts)).join("|");

  function fetchPreview(c, signal) {
    const key = previewKey(c, opts);
    if (cache.current.has(key)) return Promise.resolve(cache.current.get(key));
    if (inflight.current.has(key)) return inflight.current.get(key);
    const p = api
      .fetchBlobUrl(api.previewUrl(c.rating_key, previewParams(c, opts)), signal)
      .then((url) => {
        cache.current.set(key, url);
        return url;
      })
      .finally(() => inflight.current.delete(key));
    inflight.current.set(key, p);
    return p;
  }

  // Load the section on screen, then the next one in the background.
  useEffect(() => {
    if (!clip) {
      setSrc(null);
      return undefined;
    }
    const ctrl = new AbortController();
    let alive = true;
    setLoading(true);
    setError(null);
    fetchPreview(clip, ctrl.signal)
      .then((url) => {
        if (!alive) return;
        setSrc(url);
        setLoading(false);
        const next = sequence[index + 1];
        if (next) fetchPreview(next, ctrl.signal).catch(() => {});
      })
      .catch((e) => {
        if (!alive || e.name === "AbortError") return;
        setLoading(false);
        setError(e.message);
      });
    return () => {
      alive = false;
      ctrl.abort();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [clipKey, index]);

  // Forget previews of sections that were changed or removed.
  useEffect(() => {
    const keep = new Set(allKeys.split("|"));
    for (const [key, url] of cache.current) {
      if (!keep.has(key)) {
        URL.revokeObjectURL(url);
        cache.current.delete(key);
      }
    }
  }, [allKeys]);

  useEffect(
    () => () => {
      for (const url of cache.current.values()) URL.revokeObjectURL(url);
      cache.current.clear();
    },
    []
  );

  // The marker goes to the start of a section when it comes on screen (or to the
  // moment asked for, when the scrubber chose it).
  useEffect(() => {
    if (!spans[index]) return;
    const at = spans[index].from + pendingOffset.current;
    setElapsed(at);
    onProgress?.(at);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [index]);

  function onLoaded() {
    const v = videoRef.current;
    if (!v) return;
    if (pendingOffset.current > 0) v.currentTime = pendingOffset.current / 1000;
    pendingOffset.current = 0;
    if (playing) v.play().catch(() => setPlaying(false));
  }

  function tick() {
    const v = videoRef.current;
    if (!v || !spans[index]) return;
    const at = spans[index].from + v.currentTime * 1000;
    setElapsed(at);
    onProgress?.(at);
  }

  function ended() {
    if (index < sequence.length - 1) onIndex(index + 1);
    else setPlaying(false);
  }

  function toggle() {
    const v = videoRef.current;
    if (!v) return;
    if (v.paused) {
      setPlaying(true);
      v.play().catch(() => setPlaying(false));
    } else {
      setPlaying(false);
      v.pause();
    }
  }

  function seek(ms) {
    const at = locate(sequence, ms);
    pendingOffset.current = at.offset;
    setElapsed(ms);
    onProgress?.(ms);
    if (at.index === index) {
      const v = videoRef.current;
      if (v) v.currentTime = at.offset / 1000;
      pendingOffset.current = 0;
    } else {
      onIndex(at.index);
    }
  }

  // Space plays and pauses, unless you are typing.
  useEffect(() => {
    function onKey(e) {
      const t = e.target;
      const typing = t && (["INPUT", "TEXTAREA", "SELECT"].includes(t.tagName) || t.isContentEditable);
      if (e.code === "Space" && !typing && !e.ctrlKey && !e.metaKey) {
        e.preventDefault();
        toggle();
      }
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  if (sequence.length === 0) return null;
  return (
    <div className="seq-player">
      <div className="seq-player-stage">
        {error ? (
          <div className="placeholder preview-error">
            <div>⚠ Couldn't draw this section</div>
            <p className="error">{error}</p>
          </div>
        ) : src ? (
          <video
            key={src}
            ref={videoRef}
            src={src}
            muted={muted}
            playsInline
            autoPlay={playing}
            onLoadedData={onLoaded}
            onTimeUpdate={tick}
            onEnded={ended}
            onPlay={() => setPlaying(true)}
            onPause={() => {
              if (videoRef.current && !videoRef.current.ended) setPlaying(false);
            }}
          />
        ) : (
          <div className="placeholder">{loading ? "Drawing the section…" : "Pick a section"}</div>
        )}
        {loading && src && <span className="player-chip">Drawing section {index + 1}…</span>}
      </div>

      <div className="transport">
        <button type="button" className="tp" onClick={() => onIndex(Math.max(0, index - 1))} aria-label="Previous section" title="Previous section">
          ⏮
        </button>
        <button type="button" className="tp hot" onClick={toggle} disabled={!src} aria-label={playing ? "Pause" : "Play the sequence"} title="Play / pause (Space)">
          {playing ? "⏸" : "▶"}
        </button>
        <button
          type="button"
          className="tp"
          onClick={() => onIndex(Math.min(sequence.length - 1, index + 1))}
          aria-label="Next section"
          title="Next section"
        >
          ⏭
        </button>
        <button type="button" className="tp" onClick={() => setMuted((m) => !m)} aria-pressed={muted} title={muted ? "Sound off" : "Sound on"}>
          {muted ? "🔇" : "🔊"}
        </button>
        <span className="mono seq-player-time">
          {fmtTime(elapsed)} / {fmtTime(total)}
        </span>
        <span className="tp-spacer" />
        <span className="player-status" role="status">
          Section {index + 1} of {sequence.length}
          {clip ? ` · ${episodeLabel(clip)}` : ""}
        </span>
      </div>
      <input
        type="range"
        className="seq-scrub"
        min="0"
        max={Math.max(0, total - 1)}
        value={Math.min(elapsed, Math.max(0, total - 1))}
        onChange={(e) => seek(Number(e.target.value))}
        aria-label="Position in the sequence"
      />
    </div>
  );
}
