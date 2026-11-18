import { splitPath } from "./pathModel.js";
import { secondsText } from "./layersModel.js";

const round = (v) => Math.round(v * 1000) / 1000;
const points = (pts) => pts.map((p) => `${round(p.x * 100)},${round(p.y * 100)}`).join(" ");

/**
 * The route of everything that travels (layers and the caption text), drawn over the player's
 * picture: a line for the way already gone (solid) and the way still to go (dashed), and a dot at
 * each keyframe that jumps the playhead there. The selected layer's route is the bold one.
 * `paths` is `[{id, path, selected, label}]`; `relMs` the playhead in clip time; `pictureBox` the
 * picture's rectangle inside the player.
 */
export function PathPreview({ paths, relMs, pictureBox, onSeek }) {
  const shown = (paths || []).filter((p) => p.path && p.path.length > 0);
  if (!pictureBox || shown.length === 0) return null;
  return (
    <div
      className="path-preview"
      style={{ left: pictureBox.left, top: pictureBox.top, width: pictureBox.width, height: pictureBox.height }}
    >
      <svg viewBox="0 0 100 100" preserveAspectRatio="none" aria-hidden="true">
        {shown.map((p) => {
          const { done, ahead } = splitPath(p.path, relMs);
          const cls = `path-line${p.selected ? " selected" : ""}`;
          return (
            <g key={p.id} data-path={p.id}>
              {done.length > 1 && <polyline className={`${cls} done`} points={points(done)} />}
              {ahead.length > 1 && <polyline className={`${cls} ahead`} points={points(ahead)} />}
            </g>
          );
        })}
      </svg>
      {shown.map((p) =>
        p.path.map((k, i) => (
          <button
            key={`${p.id}-${k.t_ms}`}
            type="button"
            className={`path-key${p.selected ? " selected" : ""}${i === 0 ? " first" : ""}${i === p.path.length - 1 ? " last" : ""}`}
            style={{ left: `${k.x * 100}%`, top: `${k.y * 100}%` }}
            aria-label={`${p.label}: keyframe at ${secondsText(k.t_ms)} s`}
            title={`${p.label} at ${secondsText(k.t_ms)} s`}
            onClick={(e) => {
              e.stopPropagation();
              onSeek?.(k.t_ms);
            }}
          />
        ))
      )}
    </div>
  );
}
