import { fmtTime } from "./format.js";

const FRAME_FORMATS = [
  ["png", "PNG", "sharp, any size"],
  ["jpg", "JPG", "smallest file"],
  ["gif", "GIF", "pastes anywhere"],
];
const FRAME_WIDTHS = [480, 720, 960, 1280, 1920];

/** Take the frame the player is on, with the caption chosen above, as a picture. */
export function FrameSaver({ format, onFormat, width, onWidth, captionMode, playhead, srcFps, state, onSave, onClose }) {
  const caption =
    captionMode === "source"
      ? "with the subtitle that is on screen"
      : captionMode === "meme"
      ? "with your meme text"
      : "with no caption (choose one above to add it)";
  const frameNo = srcFps ? ` · f${Math.round((playhead * srcFps.num) / (1000 * srcFps.den))}` : "";
  return (
    <div className="frame-saver">
      <div className="frame-saver-row">
        <span className="frame-saver-title">Save this frame</span>
        <div className="radio-row" role="radiogroup" aria-label="Picture format">
          {FRAME_FORMATS.map(([id, label, hint]) => (
            <label key={id} title={hint}>
              <input type="radio" checked={format === id} onChange={() => onFormat(id)} />
              {label}
            </label>
          ))}
        </div>
        <label className="frame-width">
          Width
          <select value={width} onChange={(e) => onWidth(+e.target.value)}>
            {FRAME_WIDTHS.map((w) => (
              <option key={w} value={w}>
                {w}px
              </option>
            ))}
          </select>
        </label>
        <button type="button" className="frame-save-btn" onClick={onSave} disabled={state?.busy}>
          {state?.busy ? "Saving…" : `📷 Save frame at ${fmtTime(playhead)}${frameNo}`}
        </button>
      </div>
      <p className="hint tiny">The picture is made {caption}. Move the playhead to pick the frame.</p>
      {state?.error && (
        <p className="error">
          {state.error}{" "}
          <button type="button" className="link-btn" onClick={onClose}>
            Dismiss
          </button>
        </p>
      )}
      {state?.download_path && (
        <div className="still-out">
          <button type="button" className="still-close" onClick={onClose} aria-label="Close the saved frame" title="Close">
            ✕ Close
          </button>
          <img src={state.download_path} alt={`Frame at ${fmtTime(state.at)}`} />
          <a className="download" href={state.download_path} download={state.filename}>
            ⬇ Download {state.filename}
          </a>
        </div>
      )}
    </div>
  );
}
