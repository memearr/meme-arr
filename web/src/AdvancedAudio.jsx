import { useState } from "react";
import { api } from "./api.js";
import {
  DEFAULT_TARGET,
  HIGH_RANGE,
  LOW_RANGE,
  TARGETS,
  hzText,
  hzToStep,
  loudnessText,
  stepToHz,
} from "./advancedAudio.js";

/** A cut: a slider from off through the pitches in `range`, with its value shown. */
function Cut({ label, hz, range, onChange }) {
  return (
    <label className="adv-cut">
      <span>
        {label} <b>{hzText(hz)}</b>
      </span>
      <input
        type="range"
        min={0}
        max={100}
        step={1}
        value={hzToStep(hz, range)}
        aria-label={label}
        aria-valuetext={hzText(hz)}
        onChange={(e) => onChange(stepToHz(e.target.value, range))}
        onDoubleClick={() => onChange(0)}
      />
    </label>
  );
}

/**
 * The advanced sound tools, under the Mix: cuts for the clip's sound and the song (a high-pass takes out
 * rumble, a low-pass hiss), a loudness target for the whole mix and a reading of how loud the clip's own
 * sound is. `sound` is `{clipHigh, clipLow, normalize}` and `onSound` takes a change to it; `music` is the
 * song, if any, and `onMusic` takes a change to it. With `clipSound` false (a clip of a sequence) there is
 * only the song.
 */
export function AdvancedAudio({ ratingKey, start, end, sound, onSound, music, onMusic, clipSound = true }) {
  const [reading, setReading] = useState(null);
  const [measuring, setMeasuring] = useState(false);
  const [error, setError] = useState(null);

  async function measure() {
    setMeasuring(true);
    setError(null);
    try {
      setReading(await api.clipLoudness(ratingKey, start, end));
    } catch (e) {
      setReading(null);
      setError(e.message || "couldn't measure that");
    } finally {
      setMeasuring(false);
    }
  }

  return (
    <fieldset className="advanced-audio">
      <legend>Advanced sound</legend>
      {clipSound && (
        <div className="adv-group">
          <b>Clip sound</b>
          <Cut label="Clip high-pass" hz={sound.clipHigh} range={HIGH_RANGE} onChange={(v) => onSound({ clipHigh: v })} />
          <Cut label="Clip low-pass" hz={sound.clipLow} range={LOW_RANGE} onChange={(v) => onSound({ clipLow: v })} />
        </div>
      )}
      {music && (
        <div className="adv-group">
          <b>Song</b>
          <Cut label="Song high-pass" hz={music.highpass_hz || 0} range={HIGH_RANGE} onChange={(v) => onMusic({ highpass_hz: v })} />
          <Cut label="Song low-pass" hz={music.lowpass_hz || 0} range={LOW_RANGE} onChange={(v) => onMusic({ lowpass_hz: v })} />
        </div>
      )}
      {clipSound && (
        <div className="adv-group">
          <b>Loudness</b>
          <label className="check">
            <input
              type="checkbox"
              checked={sound.normalize != null}
              onChange={(e) => onSound({ normalize: e.target.checked ? DEFAULT_TARGET : null })}
            />{" "}
            Normalise the mix
          </label>
          {sound.normalize != null && (
            <label>
              Target{" "}
              <select
                value={sound.normalize}
                aria-label="Loudness target"
                onChange={(e) => onSound({ normalize: Number(e.target.value) })}
              >
                {TARGETS.map((t) => (
                  <option key={t.lufs} value={t.lufs}>
                    {t.label}
                  </option>
                ))}
              </select>
            </label>
          )}
          <div className="adv-reading">
            <button type="button" className="secondary" onClick={measure} disabled={measuring}>
              {measuring ? "Measuring…" : "Measure the clip's loudness"}
            </button>{" "}
            <span role="status">{error ? error : loudnessText(reading)}</span>
          </div>
        </div>
      )}
      <p className="hint tiny">
        A high-pass cuts the low end, a low-pass the top (double-click a slider for off). Normalising brings the
        whole mix to the target loudness before the master and the limiter. The player doesn't play these; the
        export has them.
      </p>
    </fieldset>
  );
}
