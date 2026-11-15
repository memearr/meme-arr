import { ValueField } from "./TrimPanel.jsx";
import { effective, overlaps } from "./cueEdit.js";
import { fmtTime } from "./format.js";
import { formatTimecode, parseTimecode } from "./timecode.js";

/** The subtitle lines that get burned in: each one editable, retimable, splittable or removable. */
export function CueEditor({
  displayCues,
  start,
  end,
  subtitleOffsetMs,
  editedCues,
  resetCues,
  srcFps,
  moveCue,
  seekTo,
  splitCueAtPlayhead,
  removeCue,
  editCueText,
  addCueAtPlayhead,
}) {
  return (
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
  );
}
