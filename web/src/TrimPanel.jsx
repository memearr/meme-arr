import { useState } from "react";
import { fmtTime } from "./format.js";
import {
  MAX_CLIP_MS,
  formatTimecode,
  frameToMs,
  framesIn,
  msToFrame,
  parseTimecode,
  stepFrames,
} from "./timecode.js";

/**
 * A text box for a time or a frame number. It follows what you type as soon as
 * it can be read (`onLive`), settles on Enter or when you leave it
 * (`onCommit`, which also tidies the text), and goes back to the current value
 * if what you typed means nothing.
 */
export function ValueField({ label, value, parse, format, onLive, onCommit, disabled, hint }) {
  const [draft, setDraft] = useState(null);
  const shown = draft ?? format(value);
  const bad = draft != null && parse(draft) == null;

  function settle() {
    if (draft != null) {
      const v = parse(draft);
      if (v != null) onCommit(v);
    }
    setDraft(null);
  }

  return (
    <label className={"trim-field" + (bad ? " bad" : "")}>
      <span className="trim-field-label">{label}</span>
      <input
        value={shown}
        disabled={disabled}
        spellCheck={false}
        inputMode="text"
        aria-invalid={bad || undefined}
        title={hint}
        onChange={(e) => {
          setDraft(e.target.value);
          const v = parse(e.target.value);
          if (v != null) onLive(v);
        }}
        onBlur={settle}
        onKeyDown={(e) => {
          if (e.key === "Enter") {
            e.preventDefault();
            settle();
            e.currentTarget.blur();
          } else if (e.key === "Escape") {
            setDraft(null);
            e.currentTarget.blur();
          }
        }}
      />
    </label>
  );
}

function NudgeRow({ fps, ms, onSet }) {
  const byFrames = (n) => (fps ? stepFrames(ms, n, fps) : Math.max(0, ms + n * 40));
  const steps = [
    ["−1s", Math.max(0, ms - 1000)],
    ["−10f", byFrames(-10)],
    ["−1f", byFrames(-1)],
    ["+1f", byFrames(1)],
    ["+10f", byFrames(10)],
    ["+1s", ms + 1000],
  ];
  return (
    <div className="trim-nudges" role="group" aria-label="Nudge">
      {steps.map(([label, to]) => (
        <button key={label} type="button" className="secondary" onClick={() => onSet(to)}>
          {label}
        </button>
      ))}
    </div>
  );
}

/**
 * Exact In/Out: a time box and a frame box for each, nudge buttons, the clip's
 * length in time and frames, and the "lock length" and "snap to" options.
 */
export function TrimPanel({
  start,
  end,
  duration,
  fps, // { num, den } or null while unknown
  playhead,
  lockLength,
  onLock,
  snap,
  onSnap,
  onTrim, // (start, end, commit, moved)
  showKeys = true, // the editor's keyboard shortcuts (not in the sequence editor)
}) {
  const length = end - start;
  const tooLong = length > MAX_CLIP_MS;
  const hint = "Type 2.4, 0:02.400, 00:00:02.400 or f58";

  function setIn(ms, commit) {
    if (lockLength) onTrim(ms, ms + length, commit, "start");
    else onTrim(ms, end, commit, "start");
  }
  function setOut(ms, commit) {
    if (lockLength) onTrim(ms - length, ms, commit, "end");
    else onTrim(start, ms, commit, "end");
  }

  const frameParse = (t) => {
    if (!fps) return null;
    const n = /^\s*f?\s*(\d+)\s*f?\s*$/i.exec(t);
    return n ? frameToMs(Number(n[1]), fps) : null;
  };
  const frameFormat = (ms) => (fps ? String(msToFrame(ms, fps)) : "–");
  const timeParse = (t) => parseTimecode(t, fps || { num: 24, den: 1 });

  return (
    <div className="trim-panel">
      <div className="trim-cols">
        <div className="trim-col">
          <div className="trim-col-head">
            <span className="trim-swatch" /> In point
          </div>
          <div className="trim-fields">
            <ValueField
              label="Time"
              value={start}
              parse={timeParse}
              format={formatTimecode}
              onLive={(v) => setIn(v, false)}
              onCommit={(v) => setIn(v, true)}
              hint={hint}
            />
            <ValueField
              label="Frame"
              value={start}
              parse={frameParse}
              format={frameFormat}
              onLive={(v) => setIn(v, false)}
              onCommit={(v) => setIn(v, true)}
              disabled={!fps}
              hint="Frame number in the source video"
            />
          </div>
          <NudgeRow fps={fps} ms={start} onSet={(v) => setIn(v, true)} />
          {playhead != null && (
            <button type="button" className="secondary" onClick={() => setIn(playhead, true)}>
              Set to playhead
            </button>
          )}
        </div>

        <div className="trim-col">
          <div className="trim-col-head">
            <span className="trim-swatch" /> Out point
          </div>
          <div className="trim-fields">
            <ValueField
              label="Time"
              value={end}
              parse={timeParse}
              format={formatTimecode}
              onLive={(v) => setOut(v, false)}
              onCommit={(v) => setOut(v, true)}
              hint={hint}
            />
            <ValueField
              label="Frame"
              value={end}
              parse={frameParse}
              format={frameFormat}
              onLive={(v) => setOut(v, false)}
              onCommit={(v) => setOut(v, true)}
              disabled={!fps}
              hint="Frame number in the source video"
            />
          </div>
          <NudgeRow fps={fps} ms={end} onSet={(v) => setOut(v, true)} />
          {playhead != null && (
            <button type="button" className="secondary" onClick={() => setOut(playhead, true)}>
              Set to playhead
            </button>
          )}
        </div>

        <div className="trim-col trim-length">
          <div className="trim-col-head">Length</div>
          <div className={"trim-length-value" + (tooLong ? " error" : "")}>
            {(length / 1000).toFixed(3)} s
          </div>
          <div className="trim-length-frames">{fps ? `${framesIn(length, fps)} frames` : fmtTime(length)}</div>
          <label className="trim-check">
            <input type="checkbox" checked={lockLength} onChange={(e) => onLock(e.target.checked)} />
            Lock length when moving In/Out
          </label>
          {duration > 0 && end >= duration && <p className="hint tiny">Out is at the end of the media.</p>}
        </div>
      </div>

      <div className="trim-foot">
        <label className="trim-snap">
          Snap to
          <select value={snap} onChange={(e) => onSnap(e.target.value)}>
            <option value="frame">Whole frames</option>
            <option value="caption">Subtitle edges</option>
            <option value="off">Nothing (to the ms)</option>
          </select>
        </label>
        {showKeys && (
        <span className="hint tiny trim-keys">
          Keys: <kbd>I</kbd>/<kbd>O</kbd> set In/Out to the playhead · <kbd>,</kbd> <kbd>.</kbd> step a frame
          (<kbd>Shift</kbd> = 10) · <kbd>Space</kbd> play · <kbd>Ctrl</kbd>+<kbd>Z</kbd> undo
        </span>
        )}
      </div>
      {fps && (
        <p className="hint tiny">
          Frames are counted in the source video ({(fps.num / fps.den).toFixed(3)} fps). Output fps
          below can differ; set it to the source rate to keep every frame.
        </p>
      )}
    </div>
  );
}
