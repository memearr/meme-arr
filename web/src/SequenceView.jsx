import { AUDIO_FORMATS } from "./clipSettings.js";
import { JobResult } from "./Editor.jsx";
import { api } from "./api.js";
import { episodeLabel, fmtTime } from "./format.js";
import { useEffect, useRef, useState } from "react";

export function SequenceView({ sequence, setSequence, onJob }) {
  const [format, setFormat] = useState("gif");
  const [width, setWidth] = useState(480);
  const [fps, setFps] = useState(15);
  const [quality, setQuality] = useState(80);
  const [audio, setAudio] = useState(false);
  const [creditSource, setCreditSource] = useState(true); // the shows/movies in the file's title tag
  const [filename, setFilename] = useState("");
  const [job, setJob] = useState(null);
  const [error, setError] = useState(null);
  const pollRef = useRef(null);

  useEffect(() => () => clearInterval(pollRef.current), []);

  function move(id, dir) {
    setSequence((seq) => {
      const i = seq.findIndex((c) => c.id === id);
      const j = i + dir;
      if (i < 0 || j < 0 || j >= seq.length) return seq;
      const copy = [...seq];
      [copy[i], copy[j]] = [copy[j], copy[i]];
      return copy;
    });
  }
  function remove(id) {
    setSequence((seq) => seq.filter((c) => c.id !== id));
  }
  function clearAll() {
    if (sequence.length && !window.confirm("Clear all clips from the sequence?")) return;
    setSequence([]);
    setJob(null);
  }

  const totalMs = sequence.reduce((sum, c) => sum + Math.max(0, c.end_ms - c.start_ms), 0);
  const tooLong = totalMs > 180_000;
  const tooMany = sequence.length > 20;

  function pushJob(j) {
    setJob(j);
    onJob?.(j);
  }

  function toggleAudio(next) {
    setAudio(next);
    if (next && !AUDIO_FORMATS.includes(format)) setFormat("mp4");
  }

  async function doExport() {
    setError(null);
    try {
      const started = await api.startSequenceExport({
        clips: sequence.map((c) => ({
          rating_key: c.rating_key,
          start_ms: c.start_ms,
          end_ms: c.end_ms,
          caption: c.caption,
          subtitle_offset_ms: c.subtitle_offset_ms || 0,
        })),
        format,
        width,
        fps,
        quality,
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
    } catch (e) {
      setError(e.message);
    }
  }

  return (
    <section className="view">
      <div className="view-head">
        <h2>Sequence</h2>
        {sequence.length > 0 && (
          <button type="button" className="danger small" onClick={clearAll}>
            Clear all
          </button>
        )}
      </div>

      {sequence.length === 0 ? (
        <p className="hint">
          No clips yet. Open a moment on the <b>Make</b> tab, trim it, and hit “🔗 Add to
          sequence” to start building a back-to-back compilation.
        </p>
      ) : (
        <>
          <ul className="seq-list">
            {sequence.map((c, i) => (
              <li key={c.id} className="seq-item">
                <span className="seq-index">{i + 1}</span>
                <div className="seq-info">
                  <div className="seq-title">{episodeLabel(c)}</div>
                  <div className="seq-sub">
                    {fmtTime(c.start_ms)}–{fmtTime(c.end_ms)} (len {fmtTime(c.end_ms - c.start_ms)})
                    {c.caption?.mode === "source" && " · burned subs"}
                    {c.caption?.mode === "source" && c.subtitle_offset_ms
                      ? ` (${c.subtitle_offset_ms > 0 ? "+" : ""}${c.subtitle_offset_ms}ms)`
                      : ""}
                    {c.caption?.mode === "meme" && " · meme text"}
                  </div>
                  {c.line && <div className="seq-line">“{c.line}”</div>}
                </div>
                <div className="seq-controls">
                  <button
                    type="button"
                    className="secondary small"
                    onClick={() => move(c.id, -1)}
                    disabled={i === 0}
                    title="Move up"
                  >
                    ▲
                  </button>
                  <button
                    type="button"
                    className="secondary small"
                    onClick={() => move(c.id, 1)}
                    disabled={i === sequence.length - 1}
                    title="Move down"
                  >
                    ▼
                  </button>
                  <button
                    type="button"
                    className="danger small"
                    onClick={() => remove(c.id)}
                    title="Remove"
                  >
                    ✕
                  </button>
                </div>
              </li>
            ))}
          </ul>

          <p className={tooLong ? "error" : "muted"}>
            {sequence.length} clip{sequence.length === 1 ? "" : "s"} · total{" "}
            {fmtTime(totalMs)}
            {tooLong && " — over the 3-minute sequence limit"}
          </p>

          <div className="controls">
            <fieldset>
              <legend>Format</legend>
              {["gif", "webp", "mp4", "webm"].map((f) => {
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
                <input
                  type="range"
                  min="240"
                  max="960"
                  step="40"
                  value={width}
                  onChange={(e) => setWidth(+e.target.value)}
                />
              </label>
              <label className="slider">
                FPS {fps}
                <input type="range" min="8" max="30" value={fps} onChange={(e) => setFps(+e.target.value)} />
              </label>
              <label className="slider">
                Quality {quality}
                <input
                  type="range"
                  min="10"
                  max="100"
                  value={quality}
                  onChange={(e) => setQuality(+e.target.value)}
                />
              </label>
            </fieldset>
          </div>

          <div className="export-row">
            <input
              className="filename"
              placeholder="compilation name (optional)"
              value={filename}
              onChange={(e) => setFilename(e.target.value)}
            />
            <button className="export" onClick={doExport} disabled={tooLong || tooMany}>
              Export {format.toUpperCase()} ({sequence.length} clips)
            </button>
          </div>
          {tooMany && <p className="error">Sequences are limited to 20 clips.</p>}
          {error && <p className="error">{error}</p>}
          {job && <JobResult job={job} />}
        </>
      )}
    </section>
  );
}
