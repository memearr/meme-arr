import { useState } from "react";
import { MAX_PATH_POINTS, mergeTracked, removeKeyframe, setKeyframe, trackLimit } from "./pathModel.js";
import { secondsText } from "./layersModel.js";

/** How wide the spot that is followed is, as a share of the picture's width. */
export const SPOT_SIZES = [
  { id: "0.06", label: "Small" },
  { id: "0.12", label: "Medium" },
  { id: "0.2", label: "Large" },
];

/**
 * "Move over time": the keyframes of something that travels across the picture. Turn it on
 * to put a first keyframe where the thing is now; then move the playhead and drag the thing
 * on the player to make the next one -- or let the editor follow it: put the thing on
 * what moves, at the playhead, and "Follow it automatically" tracks that spot of the
 * picture to the end of the clip. `place` is where the thing is at the playhead; `relMs` is
 * the playhead in clip time; `clipMs` the clip's length. `onTrack({place, relMs, size,
 * maxPoints})` asks the server to follow it, resolving to `{path, tracked_ms, lost}`.
 */
export function PathControls({ path, onChange, onSeek, onTrack, place, relMs, clipMs, what = "it" }) {
  const moving = path && path.length > 0;
  const [size, setSize] = useState("0.12");
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState(null);

  async function follow() {
    setBusy(true);
    setNote(null);
    try {
      const r = await onTrack({ place, relMs, size: Number(size), maxPoints: trackLimit(path, relMs) });
      if (!r.path || r.path.length === 0) {
        setNote({ ok: false, text: "Nothing could be followed from there." });
      } else {
        onChange(mergeTracked(path, r.path, clipMs));
        const s = secondsText(r.tracked_ms);
        setNote({
          ok: !r.lost,
          text: r.lost
            ? `Followed it for ${s} s, then lost it. Move the playhead there, put ${what} back on it, and follow again from there.`
            : `Followed it for ${s} s, to the end of the clip.`,
        });
      }
    } catch (e) {
      setNote({ ok: false, text: e.message });
    } finally {
      setBusy(false);
    }
  }

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
      {onTrack && (
        <div className="track-row">
          <label>
            Spot size
            <select value={size} onChange={(e) => setSize(e.target.value)} disabled={busy}>
              {SPOT_SIZES.map((s) => (
                <option key={s.id} value={s.id}>
                  {s.label}
                </option>
              ))}
            </select>
          </label>
          <button type="button" className="small secondary" disabled={busy} onClick={follow}>
            {busy ? "Following…" : "Follow it automatically"}
          </button>
          {note && (
            <p className={note.ok ? "hint tiny" : "error"} role="status">
              {note.text}
            </p>
          )}
          <p className="hint tiny">
            Drag {what} onto the thing that moves, at the playhead, then follow: the picture under its
            centre is tracked from here to the end of the clip. A spot with some detail works best.
          </p>
        </div>
      )}
    </div>
  );
}
