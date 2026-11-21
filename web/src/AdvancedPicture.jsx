import { ASPECTS, SPEEDS, WHOLE, cropWindow, movable, speedText } from "./advancedPicture.js";

/**
 * The advanced picture tools: the speed the clip plays at (the sound goes with it) and a reframe, a
 * window of the picture to keep (a square or a vertical one for a phone). `view` is `{aspect, cx, cy}`
 * (see `cropWindow`) and `srcAspect` the picture's width over its height. The player draws the window;
 * the captions and layers are placed on the whole picture and cropped with it.
 */
export function AdvancedPicture({ speed, onSpeed, view, onView, srcAspect, speedLocked = false }) {
  const win = cropWindow(view, srcAspect);
  const can = movable(win);
  const set = (change) => onView({ ...WHOLE, ...view, ...change });
  return (
    <fieldset className="layers-panel advanced-picture">
      <legend>Advanced picture</legend>
      <label>
        Speed{" "}
        <select value={speed} aria-label="Clip speed" disabled={speedLocked} onChange={(e) => onSpeed(Number(e.target.value))}>
          {SPEEDS.map((s) => (
            <option key={s} value={s}>
              {speedText(s)}
            </option>
          ))}
        </select>
      </label>
      <label>
        Reframe{" "}
        <select value={view?.aspect ?? "full"} aria-label="Reframe" onChange={(e) => set({ aspect: e.target.value })}>
          {ASPECTS.map((a) => (
            <option key={a.id} value={a.id}>
              {a.label}
            </option>
          ))}
        </select>
      </label>
      {can.across && (
        <label>
          Frame across <b>{Math.round((view?.cx ?? 0.5) * 100)}%</b>
          <input
            type="range"
            min={0}
            max={1}
            step={0.01}
            value={view?.cx ?? 0.5}
            aria-label="Frame position across"
            onChange={(e) => set({ cx: Number(e.target.value) })}
            onDoubleClick={() => set({ cx: 0.5 })}
          />
        </label>
      )}
      {can.down && (
        <label>
          Frame down <b>{Math.round((view?.cy ?? 0.5) * 100)}%</b>
          <input
            type="range"
            min={0}
            max={1}
            step={0.01}
            value={view?.cy ?? 0.5}
            aria-label="Frame position down"
            onChange={(e) => set({ cy: Number(e.target.value) })}
            onDoubleClick={() => set({ cy: 0.5 })}
          />
        </label>
      )}
      {speedLocked && (
        <p className="hint tiny">A section with a song of its own plays at normal speed: take its song off to change it.</p>
      )}
      <p className="hint tiny">
        Speed plays the whole clip, sound included, faster or slower; the export is shorter or longer to match.
        A reframe keeps only the lit window of the picture, at the size you export: captions and layers are placed
        on the whole picture and cropped with it, so keep them inside the window.
      </p>
    </fieldset>
  );
}
