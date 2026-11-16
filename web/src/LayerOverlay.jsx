import { useRef } from "react";
import { isVideoAsset, layerActive, layerSrc, layerStyle, pointToPlace } from "./layersModel.js";

/**
 * The layers that are on screen at the playhead, laid over the player's picture the way
 * the export will lay them over the clip. Drag one to move it; click to select it.
 * `pictureBox` is the picture's rectangle inside the player; `relMs` is the playhead in clip time.
 */
export function LayerOverlay({ layers, relMs, pictureBox, selectedId, onSelect, onMove }) {
  const box = useRef(null);
  const drag = useRef(null);
  if (!pictureBox || !layers?.length) return null;
  const visible = layers.filter((l) => layerActive(l, relMs));
  if (!visible.length) return null;

  function down(e, layer) {
    e.preventDefault();
    e.stopPropagation();
    onSelect?.(layer.id);
    drag.current = layer.id;
    e.currentTarget.setPointerCapture?.(e.pointerId);
  }
  function move(e) {
    if (!drag.current || !box.current) return;
    const place = pointToPlace(box.current.getBoundingClientRect(), e.clientX, e.clientY);
    if (place) onMove?.(drag.current, place);
  }
  function up(e) {
    drag.current = null;
    e.currentTarget.releasePointerCapture?.(e.pointerId);
  }

  return (
    <div
      ref={box}
      className="layer-overlay"
      style={{ left: pictureBox.left, top: pictureBox.top, width: pictureBox.width, height: pictureBox.height }}
    >
      {visible.map((l) => {
        const common = {
          className: `layer-item${l.id === selectedId ? " selected" : ""}`,
          style: layerStyle(l),
          onPointerDown: (e) => down(e, l),
          onPointerMove: move,
          onPointerUp: up,
          onPointerCancel: up,
          draggable: false,
          "data-layer": l.id,
        };
        return l.kind === "media" && isVideoAsset(l.asset) ? (
          <video key={l.id} src={layerSrc(l)} autoPlay loop muted playsInline {...common} />
        ) : (
          <img key={l.id} src={layerSrc(l)} alt="" {...common} />
        );
      })}
    </div>
  );
}
