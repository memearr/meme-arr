import { MAX_PATH_POINTS, removeKeyframe, setKeyframe } from "./pathModel.js";
import { secondsText } from "./layersModel.js";

/**
 * "Move over time": the keyframes of something that travels across the picture. Turn it on
 * to put a first keyframe where the thing is now; then move the playhead and drag the thing
 * on the player to make the next one. `place` is where it is at the playhead; `relMs` is the
 * playhead in clip time; `clipMs` the clip's length.
 */
export function PathControls({ path, onChange, onSeek, place, relMs, clipMs, what = "it" }) {
  const moving = path && path.length > 0;
  return (
    <div className="path-controls">
      <label>
        <input
          type="checkbox"
          checked={!!moving}
          onChange={(e) => onChange(e.target.checked ? setKeyframe([], relMs, place.x, place.y, clipMs) : [])}
        />
        Move over time
      </label>
      {moving && (
        <>
          <ul className="keyframes" aria-label="Keyframes">
            {path.map((k, i) => (
              <li key={k.t_ms}>
                <button type="button" className="link" title="Move the playhead here" onClick={() => onSeek?.(k.t_ms)}>
                  {secondsText(k.t_ms)} s
                </button>
                <span className="hint tiny">
                  {Math.round(k.x * 100)}%, {Math.round(k.y * 100)}%
                </span>
                <button
                  type="button"
                  className="link"
                  aria-label={`Remove the keyframe at ${secondsText(k.t_ms)} s`}
                  onClick={() => onChange(removeKeyframe(path, i))}
                >
                  remove
                </button>
              </li>
            ))}
          </ul>
          <div className="btn-row">
            <button
              type="button"
              className="small secondary"
              disabled={path.length >= MAX_PATH_POINTS}
              onClick={() => onChange(setKeyframe(path, relMs, place.x, place.y, clipMs))}
            >
              Keyframe at the playhead
            </button>
          </div>
          <p className="hint tiny">
            Move the playhead and drag {what} on the player to set where it is at that moment; it travels in
            a straight line between keyframes and stays at the last one.
          </p>
        </>
      )}
    </div>
  );
}
