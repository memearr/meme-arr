import { useEffect, useState } from "react";
import { api } from "./api.js";
import { SCALE_DEFAULT, SCALE_MAX, SCALE_MIN, SCALE_STEP, fontOptions } from "./fonts.js";
import { applyCaptionToAll, capLengths, nudgeAll, reverse } from "./sequenceModel.js";

/**
 * Edits that apply to every section at once: captions, font and size, moving
 * all the starts or ends, a longest length, the order. (Each action is one
 * undoable step, like every other change in the sequence.)
 */
export function SequenceBulkEdit({ sequence, onChange }) {
  const [fonts, setFonts] = useState({ fonts: [], default: null });
  const [font, setFont] = useState("");
  const [scale, setScale] = useState(SCALE_DEFAULT);
  const [cap, setCap] = useState(10);

  useEffect(() => {
    let alive = true;
    api
      .fonts()
      .then((d) => alive && setFonts(d))
      .catch(() => {});
    return () => {
      alive = false;
    };
  }, []);

  const burned = sequence.filter((c) => c.caption?.mode === "source").length;
  const withCaptions = sequence.filter((c) => c.caption && c.caption.mode !== "none").length;

  return (
    <div className="panel bulk-edit">
      <h3>All sections</h3>
      <p className="hint tiny">
        These change every section at once. {burned} of {sequence.length} burn subtitles
        {withCaptions > burned ? `, ${withCaptions - burned} have meme text` : ""}.
      </p>

      <fieldset>
        <legend>Captions</legend>
        <div className="btn-row">
          <button type="button" className="secondary small" onClick={() => onChange(applyCaptionToAll(sequence, { mode: "source" }))}>
            Burn subtitles in all
          </button>
          <button type="button" className="secondary small" onClick={() => onChange(applyCaptionToAll(sequence, { mode: "none" }))}>
            No captions in any
          </button>
        </div>
        <div className="font-controls">
          <label>
            Font
            <select value={font} onChange={(e) => setFont(e.target.value)}>
              {fontOptions(fonts.fonts, fonts.default).map((o) => (
                <option key={o.id} value={o.id}>
                  {o.label}
                </option>
              ))}
            </select>
          </label>
          <label>
            Text size <b>{scale}%</b>
            <input
              type="range"
              min={SCALE_MIN}
              max={SCALE_MAX}
              step={SCALE_STEP}
              value={scale}
              onChange={(e) => setScale(Number(e.target.value))}
            />
          </label>
          <button
            type="button"
            className="secondary small"
            disabled={withCaptions === 0}
            onClick={() => onChange(applyCaptionToAll(sequence, { font, font_scale: scale }))}
            title="Sections with no captions are left alone"
          >
            Use this font and size in all captions
          </button>
        </div>
      </fieldset>

      <fieldset>
        <legend>Timing</legend>
        <div className="nudge">
          <span>Every start</span>
          {[-500, -250, 250, 500].map((d) => (
            <button key={d} type="button" onClick={() => onChange(nudgeAll(sequence, d, 0))}>
              {d > 0 ? `+${d}` : d}
            </button>
          ))}
        </div>
        <div className="nudge">
          <span>Every end</span>
          {[-500, -250, 250, 500].map((d) => (
            <button key={d} type="button" onClick={() => onChange(nudgeAll(sequence, 0, d))}>
              {d > 0 ? `+${d}` : d}
            </button>
          ))}
        </div>
        <div className="btn-row">
          <label className="inline">
            No section longer than
            <input
              type="number"
              min="1"
              max="60"
              step="0.5"
              value={cap}
              onChange={(e) => setCap(Number(e.target.value))}
              aria-label="Longest section in seconds"
            />
            s
          </label>
          <button
            type="button"
            className="secondary small"
            disabled={!(cap > 0)}
            onClick={() => onChange(capLengths(sequence, cap * 1000))}
          >
            Shorten longer ones
          </button>
        </div>
        <p className="hint tiny">Milliseconds earlier (−) or later (+). Shorter sections lose their end.</p>
      </fieldset>

      <div className="btn-row">
        <button type="button" className="secondary small" disabled={sequence.length < 2} onClick={() => onChange(reverse(sequence))}>
          ⇅ Reverse the order
        </button>
      </div>
    </div>
  );
}
