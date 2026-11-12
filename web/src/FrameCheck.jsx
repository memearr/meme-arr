import { useEffect, useState } from "react";
import { api } from "./api.js";
import { fmtTime } from "./format.js";
import { msToFrame, stepFrames } from "./timecode.js";

/** `value` once it has stopped changing for `ms`. */
function useSettled(value, ms) {
  const [settled, setSettled] = useState(value);
  useEffect(() => {
    const t = setTimeout(() => setSettled(value), ms);
    return () => clearTimeout(t);
  }, [value, ms]);
  return settled;
}

function Shot({ ratingKey, t, fps, label, kept }) {
  const [failed, setFailed] = useState(false);
  useEffect(() => setFailed(false), [t]);
  return (
    <figure className={"frame-shot" + (kept ? " kept" : " cut")}>
      {failed ? (
        <div className="frame-missing">no frame</div>
      ) : (
        <img
          src={api.frameUrl(ratingKey, t, 240)}
          alt={`${label} at ${fmtTime(t)}`}
          onError={() => setFailed(true)}
        />
      )}
      <figcaption>
        <b>{label}</b>
        <span className="mono">
          f{msToFrame(t, fps)} · {fmtTime(t)}
        </span>
      </figcaption>
    </figure>
  );
}

/**
 * The frames either side of each cut, drawn from the source file: pick the exact
 * frame a blink, a mouth shape or a scene change starts on without a render.
 */
export function FrameCheck({ ratingKey, start, end, fps }) {
  const s = useSettled(start, 250);
  const e = useSettled(end, 250);
  if (!fps) return null;
  return (
    <div className="frame-check">
      <div className="frame-check-head">
        <h4>Cut check</h4>
        <span className="hint tiny">Whole frames from the source, either side of each cut.</span>
      </div>
      <div className="frame-check-grid">
        <Shot ratingKey={ratingKey} t={stepFrames(s, -1, fps)} fps={fps} label="Last cut" kept={false} />
        <Shot ratingKey={ratingKey} t={s} fps={fps} label="First kept" kept />
        <Shot ratingKey={ratingKey} t={stepFrames(e, -1, fps)} fps={fps} label="Last kept" kept />
        <Shot ratingKey={ratingKey} t={e} fps={fps} label="First cut" kept={false} />
      </div>
    </div>
  );
}
