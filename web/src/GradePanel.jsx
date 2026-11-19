import { PLAIN, PRESETS, TONES, adjustGrade, isPlain } from "./gradeModel.js";
import { secondsText } from "./layersModel.js";

/** One slider with its value shown. */
function Slider({ label, value, min, max, step, text, onChange }) {
  return (
    <label>
      {label} <b>{text(value)}</b>
      <input type="range" min={min} max={max} step={step} value={value} onChange={(e) => onChange(Number(e.target.value))} />
    </label>
  );
}

const plusMinus = (v) => `${v > 0 ? "+" : ""}${Math.round(v * 100)}%`;
const percent = (v) => `${Math.round(v * 100)}%`;
const seconds = (v) => `${secondsText(v)} s`;

/**
 * The Picture box: the look of the clip's picture. Colour (brightness, contrast, saturation, black and
 * white or sepia, or one of a few starting looks), a mirror image, and fades from and to black. The
 * player shows it as you go; the export does it to the clip (the fades take the captions and layers
 * with them, the colour and mirror don't touch them).
 */
export function GradePanel({ grade, onChange, clipMs }) {
  const g = { ...PLAIN, ...grade };
  const set = (changes) => onChange(adjustGrade(g, changes, clipMs));
  const plain = isPlain(g);
  return (
    <fieldset className="layers-panel grade-panel">
      <legend>Picture</legend>
      <div className="grade-presets" role="group" aria-label="Starting looks">
        {PRESETS.map((p) => (
          <button key={p.id} type="button" className="chip" onClick={() => set(p.grade)}>
            {p.label}
          </button>
        ))}
        <button type="button" className="link" onClick={() => onChange({ ...PLAIN })} disabled={plain}>
          Reset
        </button>
      </div>
      <Slider label="Brightness" value={g.brightness} min={-0.5} max={0.5} step={0.01} text={plusMinus} onChange={(v) => set({ brightness: v })} />
      <Slider label="Contrast" value={g.contrast} min={0} max={2} step={0.01} text={percent} onChange={(v) => set({ contrast: v })} />
      <Slider label="Saturation" value={g.saturation} min={0} max={3} step={0.01} text={percent} onChange={(v) => set({ saturation: v })} />
      <label>
        Tone{" "}
        <select value={g.tone} onChange={(e) => set({ tone: e.target.value })}>
          {TONES.map((t) => (
            <option key={t.id} value={t.id}>
              {t.label}
            </option>
          ))}
        </select>
      </label>
      <label className="check">
        <input type="checkbox" checked={g.mirror} onChange={(e) => set({ mirror: e.target.checked })} /> Mirror (left and right swapped)
      </label>
      <Slider label="Fade in from black" value={g.fade_in_ms} min={0} max={clipMs} step={100} text={seconds} onChange={(v) => set({ fade_in_ms: v })} />
      <Slider label="Fade out to black" value={g.fade_out_ms} min={0} max={clipMs} step={100} text={seconds} onChange={(v) => set({ fade_out_ms: v })} />
      <p className="hint tiny">
        The player is a close match, not exact. Fades take the captions and layers with them; fade the sound with
        its volume line in the Mix.
      </p>
    </fieldset>
  );
}
