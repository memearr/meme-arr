import BazarrActions from "./BazarrActions.jsx";
import SubtitleSearch from "./SubtitleSearch.jsx";
import { FrameCheck } from "./FrameCheck.jsx";
import { Timeline } from "./Timeline.jsx";
import { TrimPanel } from "./TrimPanel.jsx";
import { AUDIO_FORMATS } from "./clipSettings.js";
import { PADDING_MS } from "./clipSettings.js";
import { api } from "./api.js";
import { CaptionPanel } from "./CaptionPanel.jsx";
import { CueBrowser } from "./CueBrowser.jsx";
import { CueEditor } from "./CueEditor.jsx";
import { ExportSettings } from "./ExportSettings.jsx";
import { ExportBar } from "./ExportBar.jsx";
import { FrameSaver } from "./FrameSaver.jsx";
import { PlayerPanel } from "./PlayerPanel.jsx";
import { LayersPanel } from "./LayersPanel.jsx";
import { layersForRequest, moveLayerTo } from "./layersModel.js";
import { MusicPanel } from "./MusicPanel.jsx";
import { clipVolumeForRequest, musicForRequest } from "./musicModel.js";
import { JobResult } from "./JobResult.jsx";
import { browserStorage, loadVolume, saveVolume, typingInto } from "./editorStorage.js";
import { captionMetrics, containedRect } from "./captionPreview.js";
import { cueAt, freezeLook, lookForRequest, lookPlace, lookStyle, resolveLook, wordsAt } from "./textLook.js";
import { setKeyframe } from "./pathModel.js";
import { effective, newCue, overlaps, retimeCue, shiftCues, sortCues, splitCue, textAt } from "./cueEdit.js";
import { useBazarrConfigured } from "./bazarr.js";
import { loadFontChoice, saveFontChoice, usableFont } from "./fonts.js";
import { episodeLabel, fmtTime } from "./format.js";
import { editorSeed, updatedClip } from "./sequenceClip.js";
import {
  MAX_CLIP_MS,
  fitView,
  fpsOf,
  stepFrames,
  zoomView,
} from "./timecode.js";
import { usePreviewProxy } from "./usePreviewProxy.js";
import { useTrim } from "./useTrim.js";
import { useEffect, useRef, useState } from "react";

/**
 * `sequenceEdit` (optional) opens a clip that is already in a sequence: the
 * editor starts from that clip's trim, captions and sync, hides the output
 * settings (they belong to the whole sequence) and offers "Save to sequence"
 * in place of Export. `{ clip, index, count, output: { width, fps }, onSave(updated, andNext),
 * hasNext }`; `onClose` goes back without saving.
 */
export function Editor({ item, onJob, onClose, onAddToSequence, sequenceEdit }) {
  const seed = sequenceEdit ? editorSeed(sequenceEdit.clip) : null;
  const hasInitialTiming = item.start_ms != null && item.end_ms != null;
  const [width, setWidth] = useState(sequenceEdit?.output?.width ?? 480);
  const [fps, setFps] = useState(sequenceEdit?.output?.fps ?? 15);
  const [quality, setQuality] = useState(80);
  const [format, setFormat] = useState("webp");
  const [audio, setAudio] = useState(false);
  const [creditSource, setCreditSource] = useState(true); // the show/movie in the file's title tag
  const [captionMode, setCaptionMode] = useState(
    seed ? seed.captionMode : item.line ? "source" : "none"
  );
  // The caption font and text size (percent): remembered from last time, checked against the
  // fonts that exist now.
  const [font, setFont] = useState(() => seed?.font ?? loadFontChoice(browserStorage()).font);
  const [fontScale, setFontScale] = useState(
    () => seed?.fontScale ?? loadFontChoice(browserStorage()).scale
  );
  const [fontList, setFontList] = useState({ fonts: [], default: null });
  // How burned subtitles look (preset, colours, place, reveal, entrance); null is the plain look.
  const [look, setLook] = useState(seed?.look ?? null);
  // Shapes, stickers and clips laid over the clip (times are from the clip's start), and the
  // uploaded pictures there are to choose from.
  const [layers, setLayers] = useState([]);
  const [selectedLayer, setSelectedLayer] = useState(null);
  // A song from Navidrome under the clip, and how loud the clip's own sound is (see MusicPanel).
  const [music, setMusic] = useState(null);
  const [clipVolume, setClipVolume] = useState(1);
  const [overlayList, setOverlayList] = useState({ overlays: [], shapes: [] });
  const [top, setTop] = useState(seed ? seed.top : "");
  const [bottom, setBottom] = useState(seed ? seed.bottom : item.line || "");
  const [subtitleOffsetMs, setSubtitleOffsetMs] = useState(seed?.subtitleOffsetMs ?? 0); // sync correction, ms
  const bazarrOn = useBazarrConfigured();
  const [applying, setApplying] = useState(false);
  const [applyNote, setApplyNote] = useState(null);
  // Edited burned-subtitle lines (correct/shorten/reword/retime/split or drop a line).
  // null = use the indexed lines as-is; an array = burn exactly these instead.
  const [editedCues, setEditedCues] = useState(seed?.editedCues ?? null);
  const [filename, setFilename] = useState("");
  const [job, setJob] = useState(null);
  const [error, setError] = useState(null);
  const pollRef = useRef(null);
  // "Save this frame": the picture's format and width, and the last one made.
  const [stillFormat, setStillFormat] = useState("png");
  const [stillWidth, setStillWidth] = useState(960);
  const [still, setStill] = useState(null); // { busy } | { error } | { filename, download_path, at }

  // ---- the trim (In/Out, with undo and redo) ---------------------------------
  const [mediaDuration, setMediaDuration] = useState(null); // total runtime, once known
  const { start, end, setStart, setEnd, history, trim, undo, redo } = useTrim({
    initialStart: hasInitialTiming ? item.start_ms : 0,
    initialEnd: hasInitialTiming ? item.end_ms : 5000,
    mediaDuration,
  });

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

  // ---- the working preview -------------------------------------------------
  // One picture-only render of the clip plus some footage either side. Moving
  // the In/Out points inside it needs no new render: the player just seeks. A
  // new render is only made when the clip leaves what was rendered.
  // Sound, for the player only (not the export). The preview carries the clip's audio.
  const [volume, setVolume] = useState(() => loadVolume(browserStorage()));
  // Where the picture sits inside the player (it is letterboxed), so the caption
  // overlay can be sized and placed like the export.
  const [pictureBox, setPictureBox] = useState(null);

  // Media-wide context: total runtime + every subtitle line, so the user can
  // jump to a specific moment instead of only nudging by fixed increments.
  const [allCues, setAllCues] = useState([]);
  const [cuesReload, setCuesReload] = useState(0); // bumped when a subtitle is indexed for this item

  const clipLength = Math.max(0, end - start);
  const tooLong = clipLength > MAX_CLIP_MS;
  const {
    proxy,
    proxyLoading,
    previewError,
    setPreviewError,
    previewStalled,
    setPreviewStalled,
    previewStarted,
    videoRef,
    proxyRef,
  } = usePreviewProxy({
    ratingKey: item.rating_key,
    width,
    srcFps,
    ready,
    start,
    end,
    mediaDuration,
    tooLong,
  });

  // Indexed lines currently overlapping the clip window (offset-adjusted, so
  // a line the sync correction just pulled into view shows up here too).
  const clipCues = allCues.filter((c) => overlaps(c, start, end, subtitleOffsetMs));
  // What's actually shown for editing: the user's edits once they've started,
  // else the indexed lines as a starting point.
  const displayCues = editedCues ?? clipCues;

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

  function reloadOverlays() {
    return api
      .overlays()
      .then((d) => setOverlayList(d))
      .catch(() => {});
  }
  useEffect(() => {
    reloadOverlays();
  }, []);

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

  // Make the subtitle sync permanent: Bazarr shifts the .srt beside the video,
  // the item is re-read, and the correction now lives in the lines themselves.
  async function applyToSubtitleFile() {
    const ms = subtitleOffsetMs;
    if (!ms) return;
    const ok = window.confirm(
      `Shift the subtitle file for this video by ${ms > 0 ? "+" : ""}${ms} ms?\n\n` +
        "This changes the file itself, for everything that uses it (search results, other players), not only this clip."
    );
    if (!ok) return;
    setApplying(true);
    setApplyNote(null);
    try {
      await api.bazarrShift(item.rating_key, ms);
      // The lines now carry the shift; lines already edited here are in the
      // file's own times and move with it.
      setEditedCues((cur) => (cur ? shiftCues(cur, ms) : cur));
      setSubtitleOffsetMs(0);
      setCuesReload((n) => n + 1);
      setApplyNote({ ok: true, text: "Done: the subtitle file was shifted and re-read." });
    } catch (e) {
      setApplyNote({ ok: false, text: e.message });
    } finally {
      setApplying(false);
    }
  }

  useEffect(() => {
    saveFontChoice(browserStorage(), font, fontScale);
  }, [font, fontScale]);

  // ---- the working preview: render only when the clip leaves it --------------

  // Volume and mute apply to whichever video element is on screen.
  useEffect(() => {
    const v = videoRef.current;
    if (v) {
      v.volume = volume.level;
      v.muted = volume.muted;
    }
    saveVolume(browserStorage(), volume);
  }, [volume, proxy]);

  // Track where the picture is drawn, as the window or the layout changes.
  function measurePicture() {
    const v = videoRef.current;
    if (!v) return;
    const r = containedRect(v.clientWidth, v.clientHeight, v.videoWidth, v.videoHeight);
    setPictureBox((old) =>
      old && r && Math.abs(old.width - r.width) < 0.5 && Math.abs(old.height - r.height) < 0.5 && Math.abs(old.left - r.left) < 0.5 && Math.abs(old.top - r.top) < 0.5
        ? old
        : r
    );
  }
  useEffect(() => {
    const v = videoRef.current;
    if (!v || typeof ResizeObserver === "undefined") return undefined;
    const ro = new ResizeObserver(measurePicture);
    ro.observe(v);
    return () => ro.disconnect();
  }, [proxy]);

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

  // `frozenAt` (ms into the clip) makes a caption for a single frame: its text where its path has it then.
  function buildCaption({ frozenAt } = {}) {
    return captionMode === "none"
      ? { mode: "none" }
      : captionMode === "source"
      ? {
          mode: "source",
          cues: editedCues,
          font: font || null,
          font_scale: fontScale,
          look: lookForRequest(frozenAt != null ? freezeLook(look, frozenAt) : look, clipLength),
        }
      : {
          mode: "meme",
          top: top || null,
          bottom: bottom || null,
          font_size: 36,
          font: font || null,
          font_scale: fontScale,
        };
  }

  // Follow the spot of the picture at `place`, `relMs` into the clip, to the end of it.
  function trackSpot({ place, relMs, size, maxPoints }) {
    return api.track({
      rating_key: item.rating_key,
      start_ms: start,
      end_ms: end,
      t_ms: relMs,
      x: place.x,
      y: place.y,
      size,
      max_points: maxPoints,
    });
  }

  // Put what is on screen back into the clip it came from.
  function handleSaveToSequence(andNext) {
    sequenceEdit?.onSave(
      updatedClip(sequenceEdit.clip, {
        start,
        end,
        caption: buildCaption(),
        subtitleOffsetMs: captionMode === "source" ? subtitleOffsetMs : 0,
      }),
      andNext
    );
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

  // Choosing a song needs a format that carries sound, as keeping the clip's audio does.
  function chooseMusic(m) {
    setMusic(m);
    if (!AUDIO_FORMATS.includes(format)) setFormat("mp4");
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
        layers: layersForRequest(layers, clipLength),
        music: musicForRequest(music, clipLength),
        clip_volume: clipVolumeForRequest(clipVolume, audio),
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

  // The frame on screen, with the caption chosen above drawn on it.
  async function saveFrame() {
    const at = Math.max(0, Math.round(mediaTimeNow() ?? playhead));
    setStill({ busy: true });
    try {
      const r = await api.saveFrame({
        rating_key: item.rating_key,
        t_ms: at,
        format: stillFormat,
        width: stillWidth,
        caption: buildCaption({ frozenAt: Math.max(0, at - start) }),
        subtitle_offset_ms: captionMode === "source" ? subtitleOffsetMs : 0,
        filename: filename || null,
      });
      setStill({ ...r, at });
    } catch (err) {
      setStill({ error: err.message });
    }
  }

  // ---- what the player and timeline draw ---------------------------------------

  const overlayText =
    captionMode === "source" ? textAt(displayCues, playhead, subtitleOffsetMs) : "";
  // The caption overlay: laid over the picture itself (not the letterbox) and
  // sized the way the export sizes it.
  const metrics =
    pictureBox && captionMode !== "none"
      ? captionMetrics({
          mode: captionMode,
          width: pictureBox.width,
          height: pictureBox.height,
          scalePct: fontScale,
          baseSize: captionMode === "source" && look ? resolveLook(look).size : undefined,
          memeSize: 36,
          outWidth: width,
        })
      : null;
  const capStyle = metrics
    ? {
        layer: {
          left: pictureBox.left,
          top: pictureBox.top,
          width: pictureBox.width,
          height: pictureBox.height,
          fontSize: metrics.fontSize,
          "--cap-outline": `${metrics.outline}px`,
        },
        bottom: { bottom: metrics.margin },
        top: { top: metrics.margin },
      }
    : null;
  // The styled caption over the picture: where it sits, its colours, and (with word-by-word
  // reveal) which of its words are up at the playhead.
  const lookCue = captionMode === "source" && look ? cueAt(displayCues, playhead, subtitleOffsetMs) : null;
  const relNow = Math.max(0, Math.round(playhead - start));
  const textSpot = look ? lookPlace(look, relNow) : null;
  const lookView =
    lookCue && metrics && pictureBox
      ? {
          style: {
            ...lookStyle(textSpot ? { ...look, x: textSpot.x, y: textSpot.y } : look, pictureBox.height / 288),
            fontSize: metrics.fontSize,
            lineHeight: 1.2,
          },
          lines: wordsAt(lookCue, playhead, look.reveal),
          emphasis: 1.5,
        }
      : null;
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

      {sequenceEdit && (
        <p className="notice" role="status">
          Editing clip {sequenceEdit.index + 1} of {sequenceEdit.count} in your sequence. The format, size and
          frame rate are set for the whole sequence, so they aren't shown here.
        </p>
      )}

      <div className="editor-grid">
        <div className="editor-stage">
          <PlayerPanel
            previewError={previewError}
            setPreviewError={setPreviewError}
            proxy={proxy}
            proxyLoading={proxyLoading}
            previewStalled={previewStalled}
            videoRef={videoRef}
            measurePicture={measurePicture}
            proxyLoaded={proxyLoaded}
            playing={playing}
            setPlaying={setPlaying}
            setPlayhead={setPlayhead}
            mediaTimeNow={mediaTimeNow}
            capStyle={capStyle}
            captionMode={captionMode}
            overlayText={overlayText}
            lookView={lookView}
            pictureBox={pictureBox}
            layers={sequenceEdit ? [] : layers}
            relMs={Math.max(0, Math.round(playhead - start))}
            selectedLayer={selectedLayer}
            setSelectedLayer={setSelectedLayer}
            moveLayer={(id, place, rel) =>
              setLayers((ls) => ls.map((l) => (l.id === id ? moveLayerTo(l, place, rel ?? relNow, clipLength) : l)))
            }
            moveText={(place, rel) =>
              setLook((l) =>
                !l
                  ? l
                  : l.path?.length
                  ? { ...l, path: setKeyframe(l.path, rel ?? relNow, place.x, place.y, clipLength) }
                  : { ...l, x: place.x, y: place.y }
              )
            }
            top={top}
            bottom={bottom}
            playhead={playhead}
            srcFps={srcFps}
            seekTo={seekTo}
            start={start}
            end={end}
            stepPlayhead={stepPlayhead}
            togglePlay={togglePlay}
            speed={speed}
            setSpeed={setSpeed}
            volume={volume}
            setVolume={setVolume}
          />
          <FrameSaver
            format={stillFormat}
            onFormat={setStillFormat}
            width={stillWidth}
            onWidth={setStillWidth}
            captionMode={captionMode}
            playhead={playhead}
            srcFps={srcFps}
            state={still}
            onSave={saveFrame}
            onClose={() => setStill(null)}
          />
        </div>

        <div className="editor-main">
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
          <BazarrActions itemId={item.rating_key} onChanged={() => setCuesReload((n) => n + 1)} />

          <CueBrowser allCues={allCues} onJump={jumpToCue} />
        </div>

        <div className="editor-side">
          <div className="controls">
            <CaptionPanel
              captionMode={captionMode}
              setCaptionMode={setCaptionMode}
              font={font}
              setFont={setFont}
              fontList={fontList}
              fontScale={fontScale}
              setFontScale={setFontScale}
              look={look}
              setLook={setLook}
              relMs={Math.max(0, Math.round(playhead - start))}
              clipMs={clipLength}
              onSeek={(ms) => seekTo(start + ms)}
              onTrack={trackSpot}
              top={top}
              setTop={setTop}
              bottom={bottom}
              setBottom={setBottom}
              subtitleOffsetMs={subtitleOffsetMs}
              setSubtitleOffsetMs={setSubtitleOffsetMs}
              bazarrOn={bazarrOn}
              applying={applying}
              applyNote={applyNote}
              applyToSubtitleFile={applyToSubtitleFile}
            />

            {!sequenceEdit && (
              <LayersPanel
                layers={layers}
                setLayers={setLayers}
                selectedId={selectedLayer}
                setSelectedId={setSelectedLayer}
                onSeek={(ms) => seekTo(start + ms)}
                onTrack={trackSpot}
                overlays={overlayList.overlays}
                shapes={overlayList.shapes}
                reloadOverlays={reloadOverlays}
                clipMs={clipLength}
                relMs={Math.max(0, Math.round(playhead - start))}
              />
            )}

            {!sequenceEdit && (
              <MusicPanel
                music={music}
                onChoose={chooseMusic}
                onClear={() => setMusic(null)}
                onChange={setMusic}
                clipVolume={clipVolume}
                setClipVolume={setClipVolume}
                audio={audio}
                clipMs={clipLength}
                relMs={Math.max(0, Math.round(playhead - start))}
                playing={playing}
              />
            )}

            {!sequenceEdit && (
              <ExportSettings
                hasMusic={!!music}
                format={format}
                setFormat={setFormat}
                audio={audio}
                toggleAudio={toggleAudio}
                creditSource={creditSource}
                setCreditSource={setCreditSource}
                width={width}
                setWidth={setWidth}
                fps={fps}
                setFps={setFps}
                quality={quality}
                setQuality={setQuality}
                srcFps={srcFps}
                clipLength={clipLength}
                outFrames={outFrames}
              />
            )}
          </div>

          {captionMode === "source" && (
            <CueEditor
              displayCues={displayCues}
              start={start}
              end={end}
              subtitleOffsetMs={subtitleOffsetMs}
              editedCues={editedCues}
              resetCues={resetCues}
              srcFps={srcFps}
              moveCue={moveCue}
              seekTo={seekTo}
              splitCueAtPlayhead={splitCueAtPlayhead}
              removeCue={removeCue}
              editCueText={editCueText}
              addCueAtPlayhead={addCueAtPlayhead}
            />
          )}

          <ExportBar
            sequenceEdit={sequenceEdit}
            tooLong={tooLong}
            handleSaveToSequence={handleSaveToSequence}
            onClose={onClose}
            filename={filename}
            setFilename={setFilename}
            handleAddToSequence={handleAddToSequence}
            doExport={doExport}
            format={format}
          />
          {error && <p className="error">{error}</p>}
        </div>
      </div>

      {job && <JobResult job={job} />}
    </section>
  );
}

