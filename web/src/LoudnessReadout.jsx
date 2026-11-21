import { useEffect, useRef, useState } from "react";
import { audioContext, loudnessMeter } from "./audioGraph.js";
import { Integrator, STEP_MS, blockPower, lufsOf, lufsText } from "./loudness.js";

const SILENT = { momentary: -Infinity, integrated: -Infinity };

/**
 * A live loudness readout of the whole mix, in LUFS: the momentary reading (the last 0.4 s) and the
 * integrated one (everything since it was reset, gated as the standard says). It reads while `active`
 * (the player is playing). The reading is close to a broadcast meter's, not calibrated: the Measure
 * button reads the clip's exact figure.
 */
export function LoudnessReadout({ active }) {
  const [reading, setReading] = useState(SILENT);
  const integrator = useRef(new Integrator());
  const [clears, setClears] = useState(0);

  useEffect(() => {
    if (!active) return undefined;
    let live = true;
    let frame = 0;
    let lastBlock = 0;
    audioContext()?.resume?.().catch(() => {});
    const tick = (now) => {
      if (!live) return;
      const m = loudnessMeter();
      if (m) {
        const channels = m.channels.map((c) => {
          c.analyser.getFloatTimeDomainData(c.buffer);
          return c.buffer;
        });
        const power = blockPower(channels, m.sampleRate);
        if (now - lastBlock >= STEP_MS) {
          integrator.current.add(power);
          lastBlock = now;
        }
        setReading({ momentary: lufsOf(power), integrated: integrator.current.integrated() });
      }
      frame = requestAnimationFrame(tick);
    };
    frame = requestAnimationFrame(tick);
    return () => {
      live = false;
      cancelAnimationFrame(frame);
    };
  }, [active, clears]);

  function reset() {
    integrator.current.reset();
    setReading(SILENT);
    setClears((n) => n + 1);
  }

  return (
    <div className="loudness-readout" role="group" aria-label="Loudness">
      <span title="Loudness over the last 0.4 seconds">
        Momentary <b role="status" aria-label="Momentary loudness">{lufsText(reading.momentary)}</b>
      </span>
      <span title="Loudness of everything played since it was reset">
        Integrated <b role="status" aria-label="Integrated loudness">{lufsText(reading.integrated)}</b>
      </span>
      <button type="button" className="link" onClick={reset}>
        reset
      </button>
    </div>
  );
}
