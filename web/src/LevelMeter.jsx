import { useEffect, useRef, useState } from "react";
import { FLOOR_DB, dbText, holdPeak, isClipping, measure, meterFill, toDb } from "./mixModel.js";

// One audio graph for the page. A media element can be wired into it only once, and after that its
// sound goes out through the graph, so the wiring is remembered per element.
let context = null;
const taps = new WeakMap();

function audioContext() {
  if (context) return context;
  const Ctx = typeof window !== "undefined" ? window.AudioContext || window.webkitAudioContext : null;
  if (!Ctx) return null;
  try {
    context = new Ctx();
  } catch (_) {
    context = null;
  }
  return context;
}

/** An analyser on the sound of `el` (put in its way the first time), or null when the browser can't. */
function tap(el) {
  const known = taps.get(el);
  if (known) return known;
  const ctx = audioContext();
  if (!ctx) return null;
  try {
    const source = ctx.createMediaElementSource(el);
    const analyser = ctx.createAnalyser();
    analyser.fftSize = 1024;
    source.connect(analyser);
    analyser.connect(ctx.destination);
    const made = { analyser, buffer: new Float32Array(analyser.fftSize) };
    taps.set(el, made);
    return made;
  } catch (_) {
    return null;
  }
}

const SILENT = { rms: 0, held: FLOOR_DB, peakDb: FLOOR_DB };

/**
 * A level meter for the sound of a media element: the loudness now (RMS, the bar) and the recent
 * peak (the tick), in dB, read while `active` (the player is playing). It shows what comes out of
 * the element, so the volume, the volume line and the fades are all in it. `getEl` is asked for the
 * element on each frame, as the player's video element can be replaced.
 */
export function LevelMeter({ getEl, active, label }) {
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
      const el = ask.current?.();
      const t = el ? tap(el) : null;
      let m = { peak: 0, rms: 0 };
      if (t) {
        t.analyser.getFloatTimeDomainData(t.buffer);
        m = measure(t.buffer);
      }
      held = holdPeak(held, m.peak, now - last);
      last = now;
      setLevel({ rms: m.rms, held, peakDb: toDb(m.peak) });
      frame = requestAnimationFrame(tick);
    };
    frame = requestAnimationFrame(tick);
    return () => {
      live = false;
      cancelAnimationFrame(frame);
    };
  }, [active]);

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
