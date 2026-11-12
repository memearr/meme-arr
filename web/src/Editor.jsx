import SubtitleSearch from "./SubtitleSearch.jsx";
import { FrameCheck } from "./FrameCheck.jsx";
import { Timeline } from "./Timeline.jsx";
import { TrimPanel, ValueField } from "./TrimPanel.jsx";
import { AUDIO_FORMATS } from "./clipSettings.js";
import { PADDING_MS } from "./clipSettings.js";
import { api } from "./api.js";
import { effective, newCue, overlaps, retimeCue, sortCues, splitCue, textAt } from "./cueEdit.js";
import {
  SCALE_DEFAULT,
  SCALE_MAX,
  SCALE_MIN,
  SCALE_STEP,
  fontOptions,
  loadFontChoice,
  saveFontChoice,
  usableFont,
} from "./fonts.js";
import { episodeLabel, fmtTime } from "./format.js";
import { findCueMatches, stepMatch } from "./cueFind.js";
import {
  MAX_CLIP_MS,
  clampTrim,
  fitView,
  formatTimecode,
  fpsOf,
  fpsValue,
  parseTimecode,
  stepFrames,
  zoomView,
} from "./timecode.js";
import { useEffect, useRef, useState } from "react";

/** localStorage, or null where the browser won't give it out. */
function browserStorage() {
  try {
    return window.localStorage;
  } catch (_) {
    return null;
  }
}

/** The editor never grows a clip past what the server will preview in one go. */
const PREVIEW_MAX_MS = 60_000;
/** Footage rendered either side of the clip, so trimming wider needs no new render. */
const PROXY_PAD_MS = 3000;
/** The picture-only working copy is drawn this wide: small, so it renders fast. */
const PROXY_WIDTH = 480;
const SPEEDS = [0.25, 0.5, 1];

function Nudge({ label, onNudge }) {
  const steps = [-500, -100, 100, 500];
  return (
    <div className="nudge">
      <span>{label}</span>
      {steps.map((s) => (
        <button key={s} type="button" onClick={() => onNudge(s)}>
          {s > 0 ? `+${s}` : s}
        </button>
      ))}
    </div>
  );
}

/** True when a key press is meant for a text box, not for the editor's shortcuts. */
function typingInto(el) {
  if (!el) return false;
  const tag = el.tagName;
  return tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT" || el.isContentEditable;
}

export function Editor({ item, onJob, onClose, onAddToSequence }) {
  const hasInitialTiming = item.start_ms != null && item.end_ms != null;
  const [start, setStart] = useState(hasInitialTiming ? item.start_ms : 0);
  const [end, setEnd] = useState(hasInitialTiming ? item.end_ms : 5000);
  const [width, setWidth] = useState(480);
  const [fps, setFps] = useState(15);
  const [quality, setQuality] = useState(80);
  const [format, setFormat] = useState("webp");
  const [audio, setAudio] = useState(false);
  const [creditSource, setCreditSource] = useState(true); // the show/movie in the file's title tag
  const [captionMode, setCaptionMode] = useState(item.line ? "source" : "none");
  // The caption font and text size (percent): remembered from last time, checked against the
  // fonts that exist now.
  const [font, setFont] = useState(() => loadFontChoice(browserStorage()).font);
  const [fontScale, setFontScale] = useState(() => loadFontChoice(browserStorage()).scale);
  const [fontList, setFontList] = useState({ fonts: [], default: null });
  const [top, setTop] = useState("");
  const [bottom, setBottom] = useState(item.line || "");
  const [subtitleOffsetMs, setSubtitleOffsetMs] = useState(0); // sync correction, ms
  // Edited burned-subtitle lines (correct/shorten/reword/retime/split or drop a line).
  // null = use the indexed lines as-is; an array = burn exactly these instead.
  const [editedCues, setEditedCues] = useState(null);
  const [filename, setFilename] = useState("");
  const [job, setJob] = useState(null);
  const [error, setError] = useState(null);
  const pollRef = useRef(null);

  // ---- the file's own frame rate, the timeline and the playhead ------------
  const [videoInfo, setVideoInfo] = useState(null);
  const srcFps = fpsOf(videoInfo); // { num, den } or null until known
  const [view, setView] = useState(null); // { from, to }: the part of the media the timeline shows
  const [playhead, setPlayhead] = useState(hasInitialTiming ? item.start_ms : 0);
  const [snap, setSnap] = useState("frame");
  const [lockLength, setLockLength] = useState(false);
  const [ready, setReady] = useState(false); // the starting window has been chosen
  const [playing, setPlaying] = useState(false);
  const [speed, setSpeed] = useState(1);
  // Undo/redo of the trim: the states before and after the present one.
  const [history, setHistory] = useState({ past: [], future: [] });
  const gesture = useRef(null); // the trim as it was before the change under way

  // ---- the working preview -------------------------------------------------
  // One picture-only render of the clip plus some footage either side. Moving
  // the In/Out points inside it needs no new render: the player just seeks. A
  // new render is only made when the clip leaves what was rendered.
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
  // The exact render with the captions burned in, on request.
  const [burned, setBurned] = useState(null); // object URL
  const [burnedLoading, setBurnedLoading] = useState(false);
  const burnedUrlRef = useRef(null);
  const burnedAbort = useRef(null);

  // Media-wide context: total runtime + every subtitle line, so the user can
  // jump to a specific moment instead of only nudging by fixed increments.
  const [mediaDuration, setMediaDuration] = useState(null);
  const [allCues, setAllCues] = useState([]);
  const [cuesReload, setCuesReload] = useState(0); // bumped when a subtitle is indexed for this item
  const [showCues, setShowCues] = useState(false);
  const [cueFind, setCueFind] = useState("");
  const [findIndex, setFindIndex] = useState(-1); // position within cueMatches, not allCues
  const cueItemRefs = useRef([]);

  const clipLength = Math.max(0, end - start);
  const tooLong = clipLength > MAX_CLIP_MS;

  // Indexed lines currently overlapping the clip window (offset-adjusted, so
  // a line the sync correction just pulled into view shows up here too).
  const clipCues = allCues.filter((c) => overlaps(c, start, end, subtitleOffsetMs));
  // What's actually shown for editing: the user's edits once they've started,
  // else the indexed lines as a starting point.
  const displayCues = editedCues ?? clipCues;

  // ---- trimming ---------------------------------------------------------

  /**
   * Set the trim. `commit` false is a change still under way (a drag, a half-typed
   * time); true ends it and makes it one undo step. `moved` is the edge the
   * caller changed, which decides which one gives way if they collide.
   */
  function trim(s, e, commit = true, moved = "end") {
    const t = clampTrim(Math.round(s), Math.round(e), mediaDuration || 0, moved);
    if (!gesture.current) gesture.current = { start, end };
    setStart(t.start);
    setEnd(t.end);
    if (commit) {
      const before = gesture.current;
      gesture.current = null;
      if (before.start !== t.start || before.end !== t.end) {
        setHistory((h) => ({ past: [...h.past, before].slice(-100), future: [] }));
      }
    }
  }

  function undo() {
    setHistory((h) => {
      if (h.past.length === 0) return h;
      const prev = h.past[h.past.length - 1];
      setStart(prev.start);
      setEnd(prev.end);
      return { past: h.past.slice(0, -1), future: [{ start, end }, ...h.future] };
    });
  }
  function redo() {
    setHistory((h) => {
      if (h.future.length === 0) return h;
      const next = h.future[0];
      setStart(next.start);
      setEnd(next.end);
      return { past: [...h.past, { start, end }], future: h.future.slice(1) };
    });
  }

  // ---- editing the burned lines ---------------------------------------------

  function updateCues(fn) {
    setEditedCues((cur) => sortCues(fn(cur ?? clipCues)));
  }
  function editCueText(i, text) {
    setEditedCues((cur) => (cur ?? clipCues).map((c, j) => (j === i ? { ...c, text } : c)));
  }
  function removeCue(i) {
    setEditedCues((cur) => (cur ?? clipCues).filter((_, j) => j !== i));
  }
  /** Retime line `i` to the effective span `[s, e)`; `s === null` just ends a drag. */
  function moveCue(i, s, e) {
    if (s == null) return;
    setEditedCues((cur) =>
      (cur ?? clipCues).map((c, j) => (j === i ? retimeCue(c, s, e, subtitleOffsetMs) : c))
    );
  }
  function splitCueAtPlayhead(i) {
    updateCues((cues) => {
      const c = cues[i];
      const at =
        playhead > c.start_ms + subtitleOffsetMs && playhead < c.end_ms + subtitleOffsetMs
          ? playhead
          : (c.start_ms + c.end_ms) / 2 + subtitleOffsetMs;
      const parts = splitCue(c, at, subtitleOffsetMs);
      return parts ? [...cues.slice(0, i), ...parts, ...cues.slice(i + 1)] : cues;
    });
  }
  function addCueAtPlayhead() {
    updateCues((cues) => [...cues, newCue(playhead, subtitleOffsetMs)]);
  }
  function resetCues() {
    setEditedCues(null);
  }

  useEffect(() => {
    let alive = true;
    api
      .fonts()
      .then((d) => {
        if (!alive) return;
        setFontList(d);
        setFont((f) => usableFont(f, d.fonts));
      })
      .catch(() => {});
    return () => {
      alive = false;
    };
  }, []);

  useEffect(() => {
    saveFontChoice(browserStorage(), font, fontScale);
  }, [font, fontScale]);

  // ---- the working preview: render only when the clip leaves it --------------

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
        api.previewUrl(item.rating_key, {
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

  // The exact render, captions burned in, for checking before an export.
  async function renderBurned() {
    burnedAbort.current?.abort();
    const ctrl = new AbortController();
    burnedAbort.current = ctrl;
    setBurnedLoading(true);
    setPreviewError(null);
    try {
      const url = await api.fetchBlobUrl(
        api.previewUrl(item.rating_key, {
          start,
          end,
          width,
          fps,
          captionMode,
          top: captionMode === "meme" ? top : undefined,
          bottom: captionMode === "meme" ? bottom : undefined,
          offsetMs: captionMode === "source" ? subtitleOffsetMs : undefined,
          editedCues: captionMode === "source" ? editedCues : undefined,
          font: captionMode !== "none" ? font : undefined,
          fontScale: captionMode !== "none" ? fontScale : undefined,
        }),
        ctrl.signal
      );
      if (burnedUrlRef.current) URL.revokeObjectURL(burnedUrlRef.current);
      burnedUrlRef.current = url;
      setBurned(url);
    } catch (e) {
      if (e.name !== "AbortError") setPreviewError(e.message);
    } finally {
      if (burnedAbort.current === ctrl) setBurnedLoading(false);
    }
  }

  // Any change to the clip or its captions makes an exact render out of date.
  useEffect(() => {
    burnedAbort.current?.abort();
    setBurnedLoading(false);
    setBurned(null);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [start, end, captionMode, top, bottom, subtitleOffsetMs, editedCues, font, fontScale, width, fps]);

  // ---- opening: runtime, frame rate, subtitle lines, and a starting window --

  useEffect(() => {
    let alive = true;
    (async () => {
      let duration = null;
      try {
        const m = await api.media(item.rating_key);
        duration = m.duration_ms || null;
        if (alive) setMediaDuration(duration);
      } catch (_) {
        /* media info is just context; proceed without it */
      }
      try {
        const v = await api.videoInfo(item.rating_key);
        if (alive) setVideoInfo(v);
      } catch (_) {
        /* without it, frames fall back to plain milliseconds */
      }

      let cues = [];
      try {
        const windowEnd = duration && duration > 0 ? duration : 7_200_000; // ~2h fallback
        const d = await api.mediaSubtitles(item.rating_key, 0, windowEnd);
        cues = d.cues || [];
        if (alive) setAllCues(cues);
      } catch (_) {
        /* subtitle browsing is optional */
      }

      if (!alive) return; // closed (or another item opened) while this was loading
      if (hasInitialTiming) {
        setView(fitView(item.start_ms, item.end_ms, duration || 0));
        setReady(true);
        return;
      }

      // Pick the starting window: the first line of dialogue if the item has
      // subtitles, else the middle of the runtime.
      let win = null;
      if (cues.length > 0) {
        const first = cues[0];
        win = { start: Math.max(0, first.start_ms - PADDING_MS), end: first.end_ms + PADDING_MS };
      } else if (duration) {
        const mid = Math.max(0, Math.floor(duration / 2) - 2500);
        win = { start: mid, end: Math.min(duration, mid + 5000) };
      }
      const s = win ? win.start : 0;
      const e = win ? win.end : 5000;
      setStart(s);
      setEnd(e);
      setPlayhead(s);
      setView(fitView(s, e, duration || 0));
      setReady(true);
    })();
    return () => {
      alive = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [item.rating_key]);

  useEffect(
    () => () => {
      clearInterval(pollRef.current);
      previewAbort.current?.abort();
      burnedAbort.current?.abort();
      if (previewUrlRef.current) URL.revokeObjectURL(previewUrlRef.current);
      if (burnedUrlRef.current) URL.revokeObjectURL(burnedUrlRef.current);
    },
    []
  );

  // The timeline follows the clip when it moves out of sight (the position slider,
  // a typed time far away).
  useEffect(() => {
    if (!view) return;
    if (end < view.from || start > view.to) setView(fitView(start, end, mediaDuration || 0));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [start, end]);

  // ---- the player ------------------------------------------------------------

  // Where the proxy video is, in media time.
  function mediaTimeNow() {
    const v = videoRef.current;
    const p = proxyRef.current;
    return v && p ? p.from + v.currentTime * 1000 : null;
  }

  function seekTo(ms, keepPlaying = false) {
    const v = videoRef.current;
    const p = proxyRef.current;
    if (!v || !p) {
      setPlayhead(ms);
      return;
    }
    const t = Math.min(Math.max(ms, p.from), p.to - 1);
    v.currentTime = (t - p.from) / 1000;
    if (!keepPlaying) {
      v.pause();
      setPlaying(false);
    }
    setPlayhead(t);
  }

  function togglePlay() {
    const v = videoRef.current;
    if (!v) return;
    if (v.paused) {
      const now = mediaTimeNow();
      if (now == null || now < start || now >= end) seekTo(start, true);
      v.play().catch(() => {});
    } else {
      v.pause();
    }
  }

  function stepPlayhead(n) {
    const from = mediaTimeNow() ?? playhead;
    seekTo(srcFps ? stepFrames(from, n, srcFps) : Math.max(0, from + n * 40));
  }

  // While playing: follow the video with the playhead, and loop the clip.
  useEffect(() => {
    if (!playing) return undefined;
    let raf;
    const tick = () => {
      const v = videoRef.current;
      const p = proxyRef.current;
      if (v && p) {
        const now = p.from + v.currentTime * 1000;
        if (now >= end || now < start - 100) {
          v.currentTime = Math.max(0, (start - p.from) / 1000);
          setPlayhead(start);
        } else {
          setPlayhead(now);
        }
      }
      raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  }, [playing, start, end]);

  useEffect(() => {
    if (videoRef.current) videoRef.current.playbackRate = speed;
  }, [speed, proxy]);

  // A new render: go to the clip's start and play it.
  function proxyLoaded() {
    previewStarted.current = true;
    setPreviewStalled(false);
    const v = videoRef.current;
    const p = proxyRef.current;
    if (!v || !p) return;
    v.playbackRate = speed;
    v.currentTime = Math.max(0, (Math.max(start, p.from) - p.from) / 1000);
    setPlayhead(Math.max(start, p.from));
    v.play().catch(() => {});
  }

  // Keyboard: I / O set the points to the playhead, , . step frames, space plays.
  const shortcuts = useRef(null);
  shortcuts.current = (e) => {
    const mod = e.ctrlKey || e.metaKey;
    if (mod && !typingInto(e.target)) {
      const k = e.key.toLowerCase();
      if (k === "z") {
        e.preventDefault();
        if (e.shiftKey) redo();
        else undo();
      } else if (k === "y") {
        e.preventDefault();
        redo();
      }
      return;
    }
    if (mod || e.altKey || typingInto(e.target)) return;
    if (e.key === " " && e.target.tagName === "BUTTON") return; // a focused button's own press
    const here = mediaTimeNow() ?? playhead;
    switch (e.key.toLowerCase()) {
      case "i":
        trim(here, lockLength ? here + clipLength : end, true, "start");
        break;
      case "o":
        trim(lockLength ? here - clipLength : start, here, true, "end");
        break;
      case ",":
        stepPlayhead(e.shiftKey ? -10 : -1);
        break;
      case ".":
        stepPlayhead(e.shiftKey ? 10 : 1);
        break;
      case "<":
        stepPlayhead(-10);
        break;
      case ">":
        stepPlayhead(10);
        break;
      case " ":
        togglePlay();
        break;
      default:
        return;
    }
    e.preventDefault();
  };
  useEffect(() => {
    const onKey = (e) => shortcuts.current?.(e);
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  function pushJob(j) {
    setJob(j);
    onJob?.(j);
  }

  function jumpToCue(cue) {
    trim(Math.max(0, cue.start_ms - PADDING_MS), cue.end_ms + PADDING_MS, true, "end");
    setShowCues(false);
  }

  // A subtitle was just indexed for this item: pull the new lines in.
  useEffect(() => {
    if (cuesReload === 0) return undefined;
    let alive = true;
    const windowEnd = mediaDuration && mediaDuration > 0 ? mediaDuration : 7_200_000;
    api
      .mediaSubtitles(item.rating_key, 0, windowEnd)
      .then((d) => alive && setAllCues(d.cues || []))
      .catch(() => {});
    return () => {
      alive = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [cuesReload]);

  const cueMatches = findCueMatches(allCues, cueFind);
  const currentMatchCueIndex = findIndex >= 0 ? cueMatches[findIndex] : -1;

  useEffect(() => {
    setFindIndex(cueMatches.length > 0 ? 0 : -1);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [cueFind, allCues]);

  useEffect(() => {
    if (currentMatchCueIndex < 0) return;
    cueItemRefs.current[currentMatchCueIndex]?.scrollIntoView({
      block: "center",
      behavior: "smooth",
    });
  }, [currentMatchCueIndex]);

  function goToMatch(delta) {
    if (cueMatches.length === 0) return;
    setFindIndex((i) => stepMatch(i, delta, cueMatches.length));
  }

  function handleFindKeyDown(e) {
    if (e.key !== "Enter") return;
    e.preventDefault();
    goToMatch(e.shiftKey ? -1 : 1);
  }

  function buildCaption() {
    return captionMode === "none"
      ? { mode: "none" }
      : captionMode === "source"
      ? { mode: "source", cues: editedCues, font: font || null, font_scale: fontScale }
      : {
          mode: "meme",
          top: top || null,
          bottom: bottom || null,
          font_size: 36,
          font: font || null,
          font_scale: fontScale,
        };
  }

  function handleAddToSequence() {
    onAddToSequence?.({
      rating_key: item.rating_key,
      title: item.title,
      show: item.show,
      season: item.season,
      episode: item.episode,
      year: item.year,
      line: item.line,
      start_ms: start,
      end_ms: end,
      caption: buildCaption(),
      subtitle_offset_ms: captionMode === "source" ? subtitleOffsetMs : 0,
    });
  }

  function toggleAudio(next) {
    setAudio(next);
    // Switch off an audio-incapable format rather than leaving the radio
    // group in a state where the selected format is disabled.
    if (next && !AUDIO_FORMATS.includes(format)) setFormat("mp4");
  }

  async function doExport() {
    setError(null);
    const caption = buildCaption();
    try {
      const started = await api.startExport({
        rating_key: item.rating_key,
        start_ms: start,
        end_ms: end,
        format,
        width,
        fps,
        quality,
        caption,
        subtitle_offset_ms: captionMode === "source" ? subtitleOffsetMs : 0,
        audio,
        credit_source: creditSource,
        filename: filename || null,
      });
      pushJob(started);
      pollRef.current = setInterval(async () => {
        try {
          const j = await api.job(started.id);
          pushJob(j);
          if (j.status === "done" || j.status === "failed") clearInterval(pollRef.current);
        } catch (_) {
          clearInterval(pollRef.current);
        }
      }, 800);
    } catch (err) {
      setError(err.message);
    }
  }

  // ---- what the player and timeline draw ---------------------------------------

  const overlayText =
    captionMode === "source" ? textAt(displayCues, playhead, subtitleOffsetMs) : "";
  const timelineCues =
    captionMode === "source"
      ? [
          ...allCues
            .filter((c) => !overlaps(c, start, end, subtitleOffsetMs))
            .map((c) => ({ ...effective(c, subtitleOffsetMs), index: null })),
          ...displayCues.map((c, i) => ({ ...effective(c, subtitleOffsetMs), index: i })),
        ]
      : allCues.map((c) => ({ ...effective(c, 0), index: null }));
  const outFrames = Math.round((clipLength * fps) / 1000);
  const shownView = view || fitView(start, end, mediaDuration || 0);

  return (
    <section className="editor">
      <div className="editor-head">
        <div>
          <h2>{episodeLabel(item)}</h2>
          {item.line && <p className="line">“{item.line}”</p>}
        </div>
        <div className="editor-head-actions">
          <button
            type="button"
            className="secondary"
            onClick={undo}
            disabled={history.past.length === 0}
            title="Undo the last trim (Ctrl+Z)"
          >
            ↶ Undo
          </button>
          <button
            type="button"
            className="secondary"
            onClick={redo}
            disabled={history.future.length === 0}
            title="Redo (Ctrl+Shift+Z)"
          >
            ↷ Redo
          </button>
          <button type="button" className="editor-close" title="Close" onClick={onClose}>
            ✕
          </button>
        </div>
      </div>

      <div className="editor-grid">
        <div className="editor-main">
          <div className="player">
            <div className="preview">
              {previewError && (
                <div className="placeholder preview-error">
                  <div>⚠ Preview failed</div>
                  <p className="error">{previewError}</p>
                </div>
              )}
              {!previewError && burned && (
                <video
                  key={burned}
                  src={burned}
                  controls
                  autoPlay
                  loop
                  muted
                  onError={() => setPreviewError("The exact render couldn't be played in this browser.")}
                />
              )}
              {!previewError && !burned && proxy && (
                <div className="player-stage">
                  <video
                    key={proxy.src}
                    ref={videoRef}
                    src={proxy.src}
                    muted
                    playsInline
                    onLoadedData={proxyLoaded}
                    onPlay={() => setPlaying(true)}
                    onPause={() => setPlaying(false)}
                    onSeeked={() => {
                      if (!playing) {
                        const now = mediaTimeNow();
                        if (now != null) setPlayhead(now);
                      }
                    }}
                    onError={() =>
                      setPreviewError(
                        "The preview file was generated but this browser couldn't decode it " +
                          "(likely an unusual pixel format or codec quirk in the source). " +
                          "Exporting may still work — try a smaller width or a different format."
                      )
                    }
                  />
                  {captionMode === "source" && overlayText && (
                    <div className="cap-overlay cap-bottom">{overlayText}</div>
                  )}
                  {captionMode === "meme" && top && <div className="cap-overlay cap-top">{top}</div>}
                  {captionMode === "meme" && bottom && <div className="cap-overlay cap-bottom">{bottom}</div>}
                  <span className="player-chip mono">
                    {fmtTime(playhead)}
                    {srcFps ? ` · f${Math.round((playhead * srcFps.num) / (1000 * srcFps.den))}` : ""}
                  </span>
                </div>
              )}
              {!previewError && !burned && !proxy && (
                <div className="placeholder">{proxyLoading ? "Preparing the preview…" : "Preview will appear here"}</div>
              )}
              {previewStalled && !previewError && !burned && (
                <p className="hint preview-stalled" role="status">
                  The preview hasn't started. Move the clip a little to render it again; if it keeps happening,
                  the server log (System → Logs) says why.
                </p>
              )}
            </div>

            <div className="transport">
              <button type="button" className="tp" onClick={() => seekTo(start)} aria-label="Go to In point" title="Go to In">
                ⏮
              </button>
              <button type="button" className="tp" onClick={() => stepPlayhead(-1)} aria-label="Back one frame" title="Back one frame (,)">
                ◀
              </button>
              <button
                type="button"
                className="tp hot"
                onClick={togglePlay}
                aria-label={playing ? "Pause" : "Play the clip"}
                title="Play / pause (Space)"
                disabled={!proxy || !!burned}
              >
                {playing ? "⏸" : "▶"}
              </button>
              <button type="button" className="tp" onClick={() => stepPlayhead(1)} aria-label="Forward one frame" title="Forward one frame (.)">
                ▶
              </button>
              <button type="button" className="tp" onClick={() => seekTo(Math.max(start, end - 1))} aria-label="Go to Out point" title="Go to Out">
                ⏭
              </button>
              <label className="tp-speed">
                Speed
                <select value={speed} onChange={(e) => setSpeed(Number(e.target.value))}>
                  {SPEEDS.map((s) => (
                    <option key={s} value={s}>
                      {s}×
                    </option>
                  ))}
                </select>
              </label>
              <span className="tp-spacer" />
              <span className={"player-status" + (proxyLoading ? " busy" : "")} role="status">
                {proxyLoading ? "Updating preview…" : burned ? "Exact render" : "Preview in sync"}
              </span>
            </div>

            {captionMode !== "none" && (
              <div className="burn-check">
                {burned ? (
                  <button type="button" className="secondary" onClick={() => setBurned(null)}>
                    ← Back to editing
                  </button>
                ) : (
                  <button type="button" className="secondary" onClick={renderBurned} disabled={burnedLoading || tooLong}>
                    {burnedLoading ? "Rendering…" : "Check with burned-in captions"}
                  </button>
                )}
                <span className="hint tiny">
                  The editing preview draws captions approximately; this renders exactly what will be exported.
                </span>
              </div>
            )}
          </div>

          <Timeline
            ratingKey={item.rating_key}
            view={shownView}
            start={start}
            end={end}
            playhead={playhead}
            fps={srcFps}
            snap={snap}
            cues={timelineCues}
            onTrim={trim}
            onCueMove={(i, s, e) => moveCue(i, s, e)}
            onSeek={(ms) => seekTo(ms)}
            onZoom={(factor, center) => setView((v) => zoomView(v || shownView, factor, center, mediaDuration || 0))}
            onFit={() => setView(fitView(start, end, mediaDuration || 0))}
          />

          {mediaDuration > 0 && (
            <div className="position-seek">
              <label>
                Position in media (runtime {fmtTime(mediaDuration)})
                <input
                  type="range"
                  min="0"
                  max={mediaDuration}
                  value={start}
                  onChange={(e) => {
                    const v = +e.target.value;
                    const len = clipLength || 5000;
                    trim(v, Math.min(mediaDuration, v + len), false, "start");
                  }}
                  onPointerUp={() => trim(start, end, true)}
                  onKeyUp={() => trim(start, end, true)}
                />
              </label>
            </div>
          )}

          <TrimPanel
            start={start}
            end={end}
            duration={mediaDuration || 0}
            fps={srcFps}
            playhead={mediaTimeNow() ?? playhead}
            lockLength={lockLength}
            onLock={setLockLength}
            snap={snap}
            onSnap={setSnap}
            onTrim={trim}
          />
          {tooLong && <p className="error">Clips are limited to 60 seconds.</p>}
          {videoInfo?.variable && (
            <p className="hint">
              This file has a variable frame rate, so frame numbers are approximate; times are exact.
            </p>
          )}

          <FrameCheck ratingKey={item.rating_key} start={start} end={end} fps={srcFps} />

          <SubtitleSearch itemId={item.rating_key} onIndexed={() => setCuesReload((n) => n + 1)} />

          {allCues.length > 0 && (
            <div className="cue-browser">
              <button type="button" className="secondary" onClick={() => setShowCues((v) => !v)}>
                {showCues ? "▾" : "▸"} Browse subtitles ({allCues.length} lines)
              </button>
              {showCues && (
                <div className="cue-list-wrap">
                  <div className="cue-find-row">
                    <input
                      className="cue-filter"
                      placeholder="Find in subtitles… (Enter for next)"
                      value={cueFind}
                      onChange={(e) => setCueFind(e.target.value)}
                      onKeyDown={handleFindKeyDown}
                    />
                    <button
                      type="button"
                      className="cue-find-btn"
                      title="Previous match (Shift+Enter)"
                      disabled={cueMatches.length === 0}
                      onClick={() => goToMatch(-1)}
                    >
                      ▲
                    </button>
                    <button
                      type="button"
                      className="cue-find-btn"
                      title="Next match (Enter)"
                      disabled={cueMatches.length === 0}
                      onClick={() => goToMatch(1)}
                    >
                      ▼
                    </button>
                    {cueFind.trim() && (
                      <span className="cue-find-status">
                        {cueMatches.length > 0 ? `${findIndex + 1} of ${cueMatches.length}` : "No matches"}
                      </span>
                    )}
                  </div>
                  <ul className="cue-list">
                    {allCues.map((c, i) => (
                      <li key={i}>
                        <button
                          type="button"
                          ref={(el) => {
                            // A block, not an expression: React 19 treats a returned
                            // function as this ref's cleanup, so return nothing.
                            cueItemRefs.current[i] = el;
                          }}
                          className={"cue-item" + (i === currentMatchCueIndex ? " current-match" : "")}
                          onClick={() => jumpToCue(c)}
                        >
                          <span className="cue-ts">{fmtTime(c.start_ms)}</span>
                          <span className="cue-text">{c.text}</span>
                        </button>
                      </li>
                    ))}
                  </ul>
                </div>
              )}
            </div>
          )}
        </div>

        <div className="editor-side">
          <div className="controls">
            <fieldset>
              <legend>Captions</legend>
              <label>
                <input type="radio" checked={captionMode === "none"} onChange={() => setCaptionMode("none")} />
                None
              </label>
              <label>
                <input type="radio" checked={captionMode === "source"} onChange={() => setCaptionMode("source")} />
                Burn subtitles
              </label>
              <label>
                <input type="radio" checked={captionMode === "meme"} onChange={() => setCaptionMode("meme")} />
                Meme text
              </label>
              {captionMode !== "none" && (
                <div className="font-controls">
                  <label>
                    Font
                    <select value={font} onChange={(e) => setFont(e.target.value)}>
                      {fontOptions(fontList.fonts, fontList.default).map((o) => (
                        <option key={o.id} value={o.id}>
                          {o.label}
                        </option>
                      ))}
                    </select>
                  </label>
                  <label>
                    Text size <b>{fontScale}%</b>
                    <input
                      type="range"
                      min={SCALE_MIN}
                      max={SCALE_MAX}
                      step={SCALE_STEP}
                      value={fontScale}
                      onChange={(e) => setFontScale(Number(e.target.value))}
                    />
                  </label>
                  {fontScale !== SCALE_DEFAULT && (
                    <button type="button" className="link" onClick={() => setFontScale(SCALE_DEFAULT)}>
                      reset size
                    </button>
                  )}
                  <p className="hint tiny">Add fonts in Settings. Use “Check with burned-in captions” to see a change.</p>
                </div>
              )}
              {captionMode === "meme" && (
                <div className="meme-inputs">
                  <input placeholder="Top text" value={top} onChange={(e) => setTop(e.target.value)} />
                  <input placeholder="Bottom text" value={bottom} onChange={(e) => setBottom(e.target.value)} />
                </div>
              )}
              {captionMode === "source" && (
                <div className="sync-control">
                  <div className="sync-value">
                    Subtitle sync:{" "}
                    <b>
                      {subtitleOffsetMs > 0 ? "+" : ""}
                      {subtitleOffsetMs}ms
                    </b>
                    {subtitleOffsetMs !== 0 && (
                      <button type="button" className="link" onClick={() => setSubtitleOffsetMs(0)}>
                        reset
                      </button>
                    )}
                  </div>
                  <Nudge label="Sync" onNudge={(d) => setSubtitleOffsetMs((v) => v + d)} />
                  <p className="hint tiny">
                    Positive delays subtitles (later), negative advances them (earlier) — use if the
                    indexed timing drifts from this video. The timeline and player show the change as you make it.
                  </p>
                </div>
              )}
            </fieldset>

            <fieldset>
              <legend>Format</legend>
              {["webp", "mp4", "webm", "gif"].map((f) => {
                const disabled = audio && !AUDIO_FORMATS.includes(f);
                return (
                  <label key={f} className={disabled ? "disabled" : undefined}>
                    <input
                      type="radio"
                      checked={format === f}
                      disabled={disabled}
                      onChange={() => setFormat(f)}
                    />
                    {f.toUpperCase()}
                  </label>
                );
              })}
              <label className="audio-toggle">
                <input type="checkbox" checked={audio} onChange={(e) => toggleAudio(e.target.checked)} />
                Include audio (Opus)
              </label>
              {audio && <p className="hint tiny">GIF and WebP can't carry audio — MP4/WebM only while this is on.</p>}
              <label
                className="audio-toggle"
                title="Everything else from the original file (its name, tags, chapters, subtitle tracks) is always removed from the export."
              >
                <input type="checkbox" checked={creditSource} onChange={(e) => setCreditSource(e.target.checked)} />
                Name the source in the file's title (MP4/WebM)
              </label>
            </fieldset>

            <fieldset>
              <legend>Output</legend>
              <label className="slider">
                Width {width}px
                <input type="range" min="240" max="960" step="40" value={width} onChange={(e) => setWidth(+e.target.value)} />
              </label>
              <label className="slider">
                FPS {fps}
                <input type="range" min="8" max="30" value={fps} onChange={(e) => setFps(+e.target.value)} />
              </label>
              {srcFps && (
                <button
                  type="button"
                  className="link"
                  onClick={() => setFps(Math.min(30, Math.max(8, Math.round(fpsValue(srcFps)))))}
                  title="Use the video's own frame rate, so every source frame is kept"
                >
                  match source ({fpsValue(srcFps).toFixed(3)} fps)
                </button>
              )}
              <label className="slider">
                Quality {quality}
                <input type="range" min="10" max="100" value={quality} onChange={(e) => setQuality(+e.target.value)} />
              </label>
              <p className="hint tiny output-note">
                ≈ {outFrames} frame{outFrames === 1 ? "" : "s"} at {fps} fps · {(clipLength / 1000).toFixed(2)} s
              </p>
            </fieldset>
          </div>

          {captionMode === "source" && (
            <div className="cue-editor">
              <div className="cue-editor-head">
                <h4>Subtitle lines to burn ({displayCues.filter((c) => overlaps(c, start, end, subtitleOffsetMs)).length})</h4>
                {editedCues && (
                  <button type="button" className="link" onClick={resetCues}>
                    ↺ reset to indexed lines
                  </button>
                )}
              </div>
              <ul className="cue-edit-list">
                {displayCues.map((c, i) => {
                  if (!overlaps(c, start, end, subtitleOffsetMs)) return null;
                  const eff = effective(c, subtitleOffsetMs);
                  const parse = (t) => parseTimecode(t, srcFps || { num: 24, den: 1 });
                  return (
                    <li key={i} className="cue-edit-row">
                      <div className="cue-edit-times">
                        <ValueField
                          label="From"
                          value={eff.start_ms}
                          parse={parse}
                          format={formatTimecode}
                          onLive={(v) => moveCue(i, v, Math.max(eff.end_ms, v + 100))}
                          onCommit={(v) => moveCue(i, v, Math.max(eff.end_ms, v + 100))}
                        />
                        <ValueField
                          label="To"
                          value={eff.end_ms}
                          parse={parse}
                          format={formatTimecode}
                          onLive={(v) => moveCue(i, eff.start_ms, v)}
                          onCommit={(v) => moveCue(i, eff.start_ms, v)}
                        />
                        <button
                          type="button"
                          className="secondary small"
                          onClick={() => seekTo(eff.start_ms)}
                          title="Show the player at this line's start"
                        >
                          ⌖
                        </button>
                        <button
                          type="button"
                          className="secondary small"
                          onClick={() => splitCueAtPlayhead(i)}
                          title="Split this line at the playhead"
                        >
                          ✂
                        </button>
                        <button
                          type="button"
                          className="danger small"
                          onClick={() => removeCue(i)}
                          title="Remove this line"
                        >
                          ✕
                        </button>
                      </div>
                      <textarea
                        value={c.text}
                        aria-label={`Text of the line at ${fmtTime(eff.start_ms)}`}
                        onChange={(e) => editCueText(i, e.target.value)}
                        rows={2}
                      />
                    </li>
                  );
                })}
              </ul>
              <div className="cue-editor-actions">
                <button type="button" className="secondary" onClick={addCueAtPlayhead}>
                  + Add a line at the playhead
                </button>
              </div>
              <p className="hint tiny">
                Correct, shorten, reword or retime what gets burned in — the timeline, player and export all use
                these. Lines you edit stay put when you move the clip; “reset” brings back the indexed ones.
              </p>
              {displayCues.every((c) => !overlaps(c, start, end, subtitleOffsetMs)) && (
                <p className="hint">No subtitle lines fall inside this window, so no subtitles will be burned in.</p>
              )}
            </div>
          )}

          <div className="export-row">
            <input
              className="filename"
              placeholder="meme name (optional)"
              value={filename}
              onChange={(e) => setFilename(e.target.value)}
            />
            <button type="button" className="secondary" onClick={handleAddToSequence} disabled={tooLong}>
              🔗 Add to sequence
            </button>
            <button className="export" onClick={doExport} disabled={tooLong}>
              Export {format.toUpperCase()}
            </button>
          </div>
          {error && <p className="error">{error}</p>}
        </div>
      </div>

      {job && <JobResult job={job} />}
    </section>
  );
}

export function JobResult({ job }) {
  if (job.status === "failed") return <p className="error">Export failed: {job.error}</p>;
  if (job.status !== "done") return <p className="hint">Rendering… ({job.status})</p>;
  const url = job.download_path;
  const isVideo = job.format === "mp4" || job.format === "webm";
  return (
    <div className="result-out">
      <p>
        Done: <b>{job.filename}</b>
      </p>
      <div className="out-preview">
        {isVideo ? (
          <video src={url} controls autoPlay loop muted={!job.audio} />
        ) : (
          <img src={url} alt={job.filename} />
        )}
      </div>
      <a className="download" href={url} download={job.filename}>
        ⬇ Download {job.filename}
      </a>
    </div>
  );
}
