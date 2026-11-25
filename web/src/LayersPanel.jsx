import { useState } from "react";
import { api } from "./api.js";
import {
  FALLBACK_SHAPES,
  MAX_LAYERS,
  MAX_SIZE,
  MIN_SIZE,
  adjustLayer,
  layerLabel,
  newMediaLayer,
  newShapeLayer,
  secondsText,
} from "./layersModel.js";
import { PathControls } from "./PathControls.jsx";
import { layerPlace } from "./layersModel.js";

/** One number slider with its value. */
function Slider({ label, value, min, max, step, show, onChange }) {
  return (
    <label>
      {label} <b>{show(value)}</b>
      <input type="range" min={min} max={max} step={step} value={value} onChange={(e) => onChange(Number(e.target.value))} />
    </label>
  );
}

/**
 * The Layers box: shapes, uploaded pictures and clips laid over the clip for part of its
 * length. `clipMs` is the clip's length and `relMs` the playhead in clip time (so a new layer
 * comes on where you are).
 */
export function LayersPanel({ layers, setLayers, selectedId, setSelectedId, overlays, shapes, reloadOverlays, clipMs, relMs, onSeek, onTrack }) {
  const [error, setError] = useState(null);
  const [busy, setBusy] = useState(false);
  const full = layers.length >= MAX_LAYERS;
  const selected = layers.find((l) => l.id === selectedId) || null;
  const shapeList = shapes?.length ? shapes : FALLBACK_SHAPES;

  function add(layer) {
    setLayers((ls) => [...ls, layer]);
    setSelectedId(layer.id);
  }
  function change(id, changes) {
    setLayers((ls) => ls.map((l) => (l.id === id ? adjustLayer(l, changes, clipMs) : l)));
  }
  function remove(id) {
    setLayers((ls) => ls.filter((l) => l.id !== id));
    if (selectedId === id) setSelectedId(null);
  }
  async function upload(file) {
    if (!file) return;
    setBusy(true);
    setError(null);
    try {
      const r = await api.uploadOverlay(file);
      await reloadOverlays?.();
      add(newMediaLayer(r.overlay, { atMs: relMs, clipMs }));
    } catch (e) {
      setError(e.message);
    } finally {
      setBusy(false);
    }
  }

  return (
    <fieldset className="layers-panel" id="layers-box">
      <legend>Layers</legend>
      <div className="layer-add">
        {shapeList.map((s) => (
          <button
            key={s.id}
            type="button"
            className="small secondary"
            disabled={full}
            onClick={() => add(newShapeLayer(s.id, { atMs: relMs, clipMs }))}
          >
            + {s.label.replace(/ \(.*\)/, "")}
          </button>
        ))}
      </div>
      <div className="layer-add">
        {overlays?.length > 0 && (
          <select
            aria-label="Add an uploaded picture or clip"
            value=""
            disabled={full}
            onChange={(e) => {
              const o = overlays.find((x) => x.id === e.target.value);
              if (o) add(newMediaLayer(o, { atMs: relMs, clipMs }));
            }}
          >
            <option value="">+ Uploaded picture or clip…</option>
            {overlays.map((o) => (
              <option key={o.id} value={o.id}>
                {o.name} ({o.kind === "motion" ? "moving" : "still"})
              </option>
            ))}
          </select>
        )}
        <label className="file-btn">
          {busy ? "Uploading…" : "Upload…"}
          <input
            type="file"
            accept="image/png,image/jpeg,image/gif,image/webp,video/mp4,video/webm"
            disabled={busy || full}
            onChange={(e) => {
              void upload(e.target.files?.[0]);
              e.target.value = "";
            }}
          />
        </label>
      </div>
      {error && (
        <p className="error" role="alert">
          {error}
        </p>
      )}

      {layers.length === 0 ? (
        <p className="hint tiny">
          Add an arrow, a ring or a sticker; drag it into place on the player. Layers are in the exported
          clip or sequence (not in a saved frame).
        </p>
      ) : (
        <ul className="layer-list">
          {layers.map((l) => (
            <li key={l.id} className={l.id === selectedId ? "selected" : ""}>
              <button type="button" className="link" onClick={() => setSelectedId(l.id === selectedId ? null : l.id)}>
                {layerLabel(l, overlays, shapeList)}
              </button>
              <span className="hint tiny">
                {secondsText(l.start_ms)}–{l.end_ms == null ? "end" : secondsText(l.end_ms)} s
              </span>
              <button type="button" className="link" aria-label={`Remove ${layerLabel(l, overlays, shapeList)}`} onClick={() => remove(l.id)}>
                remove
              </button>
            </li>
          ))}
        </ul>
      )}

      {selected && (
        <div className="layer-controls">
          <div className="layer-times">
            <label>
              On at (s)
              <input
                type="number"
                min={0}
                step={0.1}
                value={secondsText(selected.start_ms)}
                onChange={(e) => change(selected.id, { start_ms: Number(e.target.value) * 1000 })}
              />
            </label>
            <button type="button" className="link" onClick={() => change(selected.id, { start_ms: relMs })}>
              = playhead
            </button>
            <label>
              Off at (s)
              <input
                type="number"
                min={0}
                step={0.1}
                value={selected.end_ms == null ? secondsText(clipMs) : secondsText(selected.end_ms)}
                onChange={(e) => change(selected.id, { end_ms: Number(e.target.value) * 1000 })}
              />
            </label>
            <button type="button" className="link" onClick={() => change(selected.id, { end_ms: relMs })}>
              = playhead
            </button>
          </div>
          <Slider label="Size" value={selected.size} min={MIN_SIZE} max={MAX_SIZE} step={0.01} show={(v) => `${Math.round(v * 100)}%`} onChange={(v) => change(selected.id, { size: v })} />
          <Slider label="Turn" value={selected.rotation} min={-180} max={180} step={5} show={(v) => `${v}°`} onChange={(v) => change(selected.id, { rotation: v })} />
          <Slider label="Opacity" value={selected.opacity} min={0.1} max={1} step={0.05} show={(v) => `${Math.round(v * 100)}%`} onChange={(v) => change(selected.id, { opacity: v })} />
          {selected.kind === "shape" && (
            <label>
              Colour
              <input type="color" value={selected.color || "#ff4500"} onChange={(e) => change(selected.id, { color: e.target.value })} />
            </label>
          )}
          <PathControls
            path={selected.path}
            onChange={(path) => change(selected.id, { path })}
            onSeek={onSeek}
            onTrack={onTrack}
            place={layerPlace(selected, relMs)}
            relMs={relMs}
            clipMs={clipMs}
            what="the layer"
          />
          <p className="hint tiny">Drag it on the player to move it.</p>
        </div>
      )}
    </fieldset>
  );
}
