import { useEffect, useRef, useState } from "react";
import { audioContext, mixMeter, tap } from "./audioGraph.js";
import { FLOOR_DB, dbText, holdPeak, isClipping, measure, meterFill, toDb } from "./mixModel.js";

const SILENT = { rms: 0, held: FLOOR_DB };

/**
 * A level meter: the loudness now (RMS, the bar) and the recent peak (the tick), in dB, read while
 * `active` (the player is playing). It meters the sound of the media element `getEl()` returns (asked
 * on each frame, as the player's video element can be replaced), or, with `master`, the whole mix.
 * It shows what comes out of the element, so the volume, the volume line, the pan and the fades are in it.
 */
export function LevelMeter({ getEl, master = false, active, label }) {
  const [level, setLevel] = useState(SILENT);
  const ask = useRef(getEl);
  ask.current = getEl;

  useEffect(() => {
    if (!active) {
      setLevel(SILENT);
      return undefined;
    }
    let live = true;
    let frame = 0;
    let held = FLOOR_DB;
    let last = performance.now();
    audioContext()?.resume?.().catch(() => {});
    const tick = (now) => {
      if (!live) return;
      let t = null;
      if (master) {
        t = mixMeter();
      } else {
        const el = ask.current?.();
        t = el ? tap(el) : null;
      }
      let m = { peak: 0, rms: 0 };
      if (t) {
        t.analyser.getFloatTimeDomainData(t.buffer);
        m = measure(t.buffer);
      }
      held = holdPeak(held, m.peak, now - last);
      last = now;
      setLevel({ rms: m.rms, held });
      frame = requestAnimationFrame(tick);
    };
    frame = requestAnimationFrame(tick);
    return () => {
      live = false;
      cancelAnimationFrame(frame);
    };
  }, [active, master]);

  const rmsDb = toDb(level.rms);
  const clip = isClipping(level.held);
  return (
    <div
      className={"level-meter" + (clip ? " clip" : "")}
      role="meter"
      aria-label={`${label} meter`}
      aria-valuemin={FLOOR_DB}
      aria-valuemax={0}
      aria-valuenow={Math.round(rmsDb)}
      aria-valuetext={dbText(rmsDb)}
      title={`${label}: ${dbText(rmsDb)}, peak ${dbText(level.held)}`}
    >
      <div className="level-fill" style={{ width: `${meterFill(rmsDb) * 100}%` }} />
      <div className="level-peak" style={{ left: `${meterFill(level.held) * 100}%` }} />
    </div>
  );
}
