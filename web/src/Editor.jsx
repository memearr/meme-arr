import SubtitleSearch from "./SubtitleSearch.jsx";
import { AUDIO_FORMATS } from "./clipSettings.js";
import { PADDING_MS } from "./clipSettings.js";
import { api } from "./api.js";
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
import { useEffect, useRef, useState } from "react";

/** localStorage, or null where the browser won't give it out. */
function browserStorage() {
  try {
    return window.localStorage;
  } catch (_) {
    return null;
  }
}

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
  // Edited burned-subtitle lines (correct/shorten/reword or drop a line).
  // null = use the indexed lines as-is; an array = burn exactly these instead.
  const [editedCues, setEditedCues] = useState(null);
  const [filename, setFilename] = useState("");
  const [previewSrc, setPreviewSrc] = useState(null);
  const [previewLoading, setPreviewLoading] = useState(false);
  const [previewError, setPreviewError] = useState(null);
  const [job, setJob] = useState(null);
  const [error, setError] = useState(null);
  const pollRef = useRef(null);
  const previewUrlRef = useRef(null); // current object URL, for cleanup
  const previewSeq = useRef(0); // which preview request is the latest
  const previewAbort = useRef(null); // lets a newer request cancel an older one
  const previewStarted = useRef(false); // has the current preview begun to play?
  const [previewStalled, setPreviewStalled] = useState(false);

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
  const tooLong = clipLength > 60000;

  // Indexed lines currently overlapping the clip window (offset-adjusted, so
  // a line the sync correction just pulled into view shows up here too).
  const clipCues = allCues.filter((c) => {
    const s = c.start_ms + subtitleOffsetMs;
    const e = c.end_ms + subtitleOffsetMs;
    return e > start && s < end;
  });
  // What's actually shown for editing: the user's edits once they've started,
  // else the indexed lines as a starting point.
  const displayCues = editedCues ?? clipCues;

  function editCueText(i, text) {
    setEditedCues((cur) => {
      const base = cur ?? clipCues;
      return base.map((c, j) => (j === i ? { ...c, text } : c));
    });
  }
  function removeCue(i) {
    setEditedCues((cur) => {
      const base = cur ?? clipCues;
      return base.filter((_, j) => j !== i);
    });
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

  // Trimming the clip invalidates any in-progress edits (the set of lines in
  // view may have changed) — start clean rather than risk stale/mismatched text.
  useEffect(() => {
    setEditedCues(null);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [start, end]);

  // `win` ({start, end}) is for a caller that has just picked a window whose
  // state hasn't been applied yet; as a click handler it receives an event,
  // which is ignored.
  async function loadPreview(win) {
    const wStart = win && typeof win.start === "number" ? win.start : start;
    const wEnd = win && typeof win.end === "number" ? win.end : end;
    // A newer request replaces an older one: cancel it (the server stops
    // rendering it) and never let its answer arrive after the newer one's.
    previewAbort.current?.abort();
    const ctrl = new AbortController();
    previewAbort.current = ctrl;
    const seq = ++previewSeq.current;
    setPreviewLoading(true);
    setPreviewError(null);
    setPreviewStalled(false);
    previewStarted.current = false;
    try {
      const url = await api.fetchBlobUrl(
        api.previewUrl(item.rating_key, {
          start: wStart,
          end: wEnd,
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
      if (seq !== previewSeq.current) {
        URL.revokeObjectURL(url); // superseded while it was loading
        return;
      }
      if (previewUrlRef.current) URL.revokeObjectURL(previewUrlRef.current);
      previewUrlRef.current = url;
      setPreviewSrc(url);
    } catch (e) {
      if (e.name === "AbortError" || seq !== previewSeq.current) return;
      setPreviewSrc(null);
      setPreviewError(e.message);
    } finally {
      if (seq === previewSeq.current) setPreviewLoading(false);
    }
  }

  // A preview that is on screen but never starts (the thing that looks like a
  // button that does nothing): say so after a few seconds instead of leaving it.
  useEffect(() => {
    if (!previewSrc) return undefined;
    const t = setTimeout(() => {
      if (!previewStarted.current) setPreviewStalled(true);
    }, 6000);
    return () => clearTimeout(t);
  }, [previewSrc]);

  // On open: fetch the item's total runtime, and — if we weren't handed a
  // specific subtitle moment (opened from Library/System rather than a
  // dialogue search hit) — pick a sensible starting point: the first line of
  // dialogue if the item has subtitles, else the middle of the runtime.
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
        loadPreview();
        return;
      }

      // Pick the starting window and preview *that* window: the state set here
      // isn't visible to loadPreview yet, so it is passed in.
      let win = null;
      if (cues.length > 0) {
        const first = cues[0];
        win = { start: Math.max(0, first.start_ms - PADDING_MS), end: first.end_ms + PADDING_MS };
      } else if (duration) {
        const mid = Math.max(0, Math.floor(duration / 2) - 2500);
        win = { start: mid, end: Math.min(duration, mid + 5000) };
      }
      if (win) {
        setStart(win.start);
        setEnd(win.end);
      }
      loadPreview(win || undefined);
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
      if (previewUrlRef.current) URL.revokeObjectURL(previewUrlRef.current);
    },
    []
  );

  function pushJob(j) {
    setJob(j);
    onJob?.(j);
  }

  function jumpToCue(cue) {
    setStart(Math.max(0, cue.start_ms - PADDING_MS));
    setEnd(cue.end_ms + PADDING_MS);
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

  return (
    <section className="editor">
      <div className="editor-head">
        <div>
          <h2>{episodeLabel(item)}</h2>
          {item.line && <p className="line">“{item.line}”</p>}
        </div>
        <button type="button" className="editor-close" title="Close" onClick={onClose}>
          ✕
        </button>
      </div>

      <div className="preview">
        {previewLoading && <div className="placeholder">Loading preview…</div>}
        {!previewLoading && previewError && (
          <div className="placeholder preview-error">
            <div>⚠ Preview failed</div>
            <p className="error">{previewError}</p>
          </div>
        )}
        {!previewLoading && !previewError && previewSrc && (
          <video
            src={previewSrc}
            controls
            autoPlay
            loop
            muted
            onCanPlay={() => {
              previewStarted.current = true;
              setPreviewStalled(false);
            }}
            onError={() =>
              setPreviewError(
                "The preview file was generated but this browser couldn't decode it " +
                  "(likely an unusual pixel format or codec quirk in the source). " +
                  "Exporting may still work — try a smaller width or a different format."
              )
            }
          />
        )}
        {!previewLoading && !previewError && !previewSrc && (
          <div className="placeholder">Preview will appear here</div>
        )}
        {previewStalled && !previewError && (
          <p className="hint preview-stalled" role="status">
            The preview hasn't started. Try <b>Reload preview</b> below; if it keeps happening, the server log
            (System → Logs) says why.
          </p>
        )}
      </div>

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
                setStart(v);
                setEnd(Math.min(mediaDuration, v + len));
              }}
              onMouseUp={loadPreview}
              onTouchEnd={loadPreview}
            />
          </label>
        </div>
      )}

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

      <div className="timing">
        <div className="times">
          <span>
            In <b>{fmtTime(start)}</b>
          </span>
          <span>
            Out <b>{fmtTime(end)}</b>
          </span>
          <span className={tooLong ? "error" : ""}>Length {fmtTime(clipLength)}</span>
        </div>
        <Nudge label="In" onNudge={(d) => setStart((v) => Math.max(0, v + d))} />
        <Nudge label="Out" onNudge={(d) => setEnd((v) => Math.max(start + 100, v + d))} />
        <button type="button" className="reload" onClick={loadPreview}>
          ↻ Reload preview
        </button>
      </div>

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
              <p className="hint tiny">Add fonts in Settings. Reload preview to see a change.</p>
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
                indexed timing drifts from this video. Reload preview to check it.
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
            <input
              type="checkbox"
              checked={audio}
              onChange={(e) => toggleAudio(e.target.checked)}
            />
            Include audio (Opus)
          </label>
          {audio && (
            <p className="hint tiny">GIF and WebP can't carry audio — MP4/WebM only while this is on.</p>
          )}
          <label className="audio-toggle" title="Everything else from the original file (its name, tags, chapters, subtitle tracks) is always removed from the export.">
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
          <label className="slider">
            Quality {quality}
            <input type="range" min="10" max="100" value={quality} onChange={(e) => setQuality(+e.target.value)} />
          </label>
        </fieldset>
      </div>

      {captionMode === "source" && displayCues.length > 0 && (
        <div className="cue-editor">
          <div className="cue-editor-head">
            <h4>Subtitle lines to burn ({displayCues.length})</h4>
            {editedCues && (
              <button type="button" className="link" onClick={resetCues}>
                ↺ reset to indexed text
              </button>
            )}
          </div>
          <ul className="cue-edit-list">
            {displayCues.map((c, i) => (
              <li key={i}>
                <span className="cue-edit-ts">{fmtTime(c.start_ms)}</span>
                <textarea
                  value={c.text}
                  onChange={(e) => editCueText(i, e.target.value)}
                  rows={1}
                />
                <button
                  type="button"
                  className="danger small"
                  onClick={() => removeCue(i)}
                  title="Remove this line"
                >
                  ✕
                </button>
              </li>
            ))}
          </ul>
          <p className="hint tiny">
            Correct, shorten, or reword what gets burned in — applies to the preview and export.
            Trimming the in/out points resets this list.
          </p>
        </div>
      )}
      {captionMode === "source" && displayCues.length === 0 && (
        <p className="hint">No subtitle lines fall inside this window, so no subtitles will be burned in.</p>
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
      {tooLong && <p className="error">Clips are limited to 60 seconds.</p>}
      {error && <p className="error">{error}</p>}

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
