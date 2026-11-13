import { AUDIO_FORMATS } from "./clipSettings.js";
import { Editor, JobResult } from "./Editor.jsx";
import { SectionEditor } from "./SectionEditor.jsx";
import { SequenceBulkEdit } from "./SequenceBulkEdit.jsx";
import SequencePlayer from "./SequencePlayer.jsx";
import { SequenceTimeline } from "./SequenceTimeline.jsx";
import { replaceClip } from "./sequenceClip.js";
import {
  MAX_SECTIONS,
  MAX_TOTAL_MS,
  duplicate,
  move,
  problems,
  remove,
  totalMs,
  trim,
} from "./sequenceModel.js";
import { api } from "./api.js";
import { useEffect, useRef, useState } from "react";

function newSectionId() {
  return `${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
}

/**
 * The Sequence tab: the whole set of sections as one thing to edit. Play it
 * back to back, see it as a strip, reorder, trim or caption one section or all
 * of them, then export. A section's full editor is one click away.
 */
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
  const [editingId, setEditingId] = useState(null); // the clip open in the full editor
  const [selectedId, setSelectedId] = useState(null); // the section being edited and shown
  const [progressMs, setProgressMs] = useState(0);
  // Undo / redo of the edits made here (not of clips added from elsewhere).
  const [history, setHistory] = useState({ past: [], future: [] });
  const lastGroup = useRef({ key: null, at: 0 });

  useEffect(() => () => clearInterval(pollRef.current), []);

  // The selection follows the set: a first section is picked, a removed one is dropped.
  const selectedIndex = sequence.findIndex((c) => c.id === selectedId);
  useEffect(() => {
    if (sequence.length === 0) {
      if (selectedId !== null) setSelectedId(null);
    } else if (selectedIndex < 0) {
      setSelectedId(sequence[0].id);
    }
  }, [sequence, selectedId, selectedIndex]);
  const index = Math.max(0, selectedIndex);
  const selected = sequence[index];

  /**
   * Change the sequence as one undoable step. Edits in a row with the same
   * `group` within a second (typing a time, dragging a slider) are one step.
   */
  function commit(next, group = null) {
    const now = Date.now();
    const joined = group && lastGroup.current.key === group && now - lastGroup.current.at < 1000;
    lastGroup.current = { key: group, at: now };
    if (!joined) {
      setHistory((h) => ({ past: [...h.past.slice(-49), sequence], future: [] }));
    }
    setSequence(next);
  }
  function undo() {
    setHistory((h) => {
      if (h.past.length === 0) return h;
      const prev = h.past[h.past.length - 1];
      setSequence(prev);
      return { past: h.past.slice(0, -1), future: [sequence, ...h.future] };
    });
    lastGroup.current = { key: null, at: 0 };
  }
  function redo() {
    setHistory((h) => {
      if (h.future.length === 0) return h;
      const [next, ...rest] = h.future;
      setSequence(next);
      return { past: [...h.past, sequence], future: rest };
    });
    lastGroup.current = { key: null, at: 0 };
  }

  // One clip open in the full editor: the same trimming, captions and subtitle
  // lines as when it was made, saved back into its place in the sequence.
  const editingIndex = sequence.findIndex((c) => c.id === editingId);
  if (editingIndex >= 0) {
    const clip = sequence[editingIndex];
    return (
      <section className="view">
        <Editor
          key={clip.id}
          item={clip}
          onJob={() => {}}
          onClose={() => setEditingId(null)}
          sequenceEdit={{
            clip,
            index: editingIndex,
            count: sequence.length,
            output: { width, fps },
            hasNext: editingIndex < sequence.length - 1,
            onSave: (updated, andNext) => {
              commit(replaceClip(sequence, updated));
              setEditingId(andNext ? sequence[editingIndex + 1].id : null);
              setSelectedId(andNext ? sequence[editingIndex + 1].id : updated.id);
            },
          }}
        />
      </section>
    );
  }

  function clearAll() {
    if (sequence.length && !window.confirm("Clear all clips from the sequence?")) return;
    commit([]);
    setJob(null);
  }

  const total = totalMs(sequence);
  const blocking = problems(sequence);
  const tooLong = total > MAX_TOTAL_MS;
  const tooMany = sequence.length > MAX_SECTIONS;

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
          <div className="editor-head-actions">
            <button type="button" className="secondary" onClick={undo} disabled={history.past.length === 0} title="Undo the last change to the sequence">
              ↶ Undo
            </button>
            <button type="button" className="secondary" onClick={redo} disabled={history.future.length === 0} title="Redo">
              ↷ Redo
            </button>
            <button type="button" className="danger small" onClick={clearAll}>
              Clear all
            </button>
          </div>
        )}
      </div>

      {sequence.length === 0 ? (
        <p className="hint">
          No clips yet. Open a moment on the <b>Make</b> tab, trim it, and hit “🔗 Add to
          sequence” to start building a back-to-back compilation.
        </p>
      ) : (
        <>
          <SequencePlayer
            sequence={sequence}
            width={width}
            fps={fps}
            index={index}
            onIndex={(i) => sequence[i] && setSelectedId(sequence[i].id)}
            onProgress={setProgressMs}
          />

          <SequenceTimeline
            sequence={sequence}
            selectedId={selected?.id}
            progressMs={progressMs}
            onSelect={setSelectedId}
            onMove={(from, to) => commit(move(sequence, from, to))}
            onDuplicate={(id) => commit(duplicate(sequence, id, newSectionId()))}
            onRemove={(id) => commit(remove(sequence, id))}
          />
          {blocking.map((m) => (
            <p key={m} className="error">
              {m}
            </p>
          ))}

          <div className="seq-edit-grid">
            {selected && (
              <SectionEditor
                clip={selected}
                index={index}
                count={sequence.length}
                onChange={(updated) => commit(replaceClip(sequence, updated), `section:${updated.id}`)}
                onDetailed={() => setEditingId(selected.id)}
              />
            )}
            <SequenceBulkEdit sequence={sequence} onChange={(next) => commit(next)} />
          </div>

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
