import { SCALE_DEFAULT, SCALE_MAX, SCALE_MIN, SCALE_STEP, fontOptions } from "./fonts.js";
import { LookControls } from "./LookControls.jsx";

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

/** The Captions box: none, burned subtitles or meme text; font, size, and the subtitle sync. */
export function CaptionPanel({
  captionMode,
  setCaptionMode,
  font,
  setFont,
  fontList,
  fontScale,
  setFontScale,
  look,
  setLook,
  relMs = 0,
  clipMs = 0,
  onSeek,
  top,
  setTop,
  bottom,
  setBottom,
  subtitleOffsetMs,
  setSubtitleOffsetMs,
  bazarrOn,
  applying,
  applyNote,
  applyToSubtitleFile,
}) {
  return (
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
          <p className="hint tiny">
            Add fonts in Settings. The player shows the size and position the export will use; the
            typeface itself is only matched in the exported file.
          </p>
        </div>
      )}
      {captionMode === "meme" && (
        <div className="meme-inputs">
          <input placeholder="Top text" value={top} onChange={(e) => setTop(e.target.value)} />
          <input placeholder="Bottom text" value={bottom} onChange={(e) => setBottom(e.target.value)} />
        </div>
      )}
      {captionMode === "source" && (
        <LookControls look={look} setLook={setLook} relMs={relMs} clipMs={clipMs} onSeek={onSeek} />
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
          {bazarrOn && (
            <div className="btn-row">
              <button
                type="button"
                className="small secondary"
                disabled={subtitleOffsetMs === 0 || applying}
                onClick={applyToSubtitleFile}
                title="Ask Bazarr to shift the subtitle file itself by this amount"
              >
                {applying ? "Applying…" : "Apply to the subtitle file (Bazarr)"}
              </button>
            </div>
          )}
          {applyNote && (
            <p className={applyNote.ok ? "hint tiny" : "error"} role="status">
              {applyNote.text}
            </p>
          )}
          <p className="hint tiny">
            Positive delays subtitles (later), negative advances them (earlier) — use if the
            indexed timing drifts from this video. The timeline and player show the change as you make it.
          </p>
        </div>
      )}
    </fieldset>
  );
}
