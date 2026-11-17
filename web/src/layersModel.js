// Layers: pictures laid over the clip for part of its length -- a built-in shape (an
// arrow, a ring...), an uploaded sticker, or another clip. The server draws them
// (src/export/layers.rs); this file is the editor's side: making and adjusting layers,
// what goes into the export request, and where one sits over the player's picture.
//
// A layer's times are in milliseconds from the start of the clip. Its place is the
// centre as fractions of the picture (0 left/top .. 1 right/bottom); its size is its
// width as a fraction of the picture's width.

import { withBase } from "./base.js";
import { pathForRequest, positionAt, setKeyframe, tidyPath } from "./pathModel.js";

export const MAX_LAYERS = 12;
export const MIN_SIZE = 0.02;
export const MAX_SIZE = 1.5;
/** How long a new layer is on for, unless the clip ends first. */
export const DEFAULT_LAYER_MS = 2000;

/** The shapes the server draws, for when its list has not loaded yet. */
export const FALLBACK_SHAPES = [
  { id: "arrow", label: "Arrow (up)" },
  { id: "ring", label: "Ring" },
  { id: "box", label: "Box" },
  { id: "dot", label: "Dot" },
];

let counter = 0;
function newId() {
  counter += 1;
  return `layer-${Date.now().toString(36)}-${counter}`;
}

const clamp = (v, lo, hi) => Math.min(hi, Math.max(lo, v));

function timing(atMs, clipMs) {
  const start = clamp(Math.round(atMs), 0, Math.max(0, clipMs - 100));
  return { start_ms: start, end_ms: Math.min(clipMs, start + DEFAULT_LAYER_MS) };
}

const BASE = { x: 0.5, y: 0.5, size: 0.2, rotation: 0, opacity: 1 };

/** A new shape layer, on from `atMs` (clip time) for a couple of seconds. */
export function newShapeLayer(shape, { atMs = 0, clipMs }) {
  return { id: newId(), kind: "shape", shape, ...BASE, ...timing(atMs, clipMs) };
}

/** A new layer showing an uploaded picture or clip (`overlay` is its `GET /api/overlays` entry). */
export function newMediaLayer(overlay, { atMs = 0, clipMs }) {
  return { id: newId(), kind: "media", asset: overlay.id, ...BASE, size: 0.3, ...timing(atMs, clipMs) };
}

/** The layer with `changes` applied, kept inside what the server accepts. */
export function adjustLayer(layer, changes, clipMs) {
  const next = { ...layer, ...changes };
  next.x = clamp(Number(next.x), 0, 1);
  next.y = clamp(Number(next.y), 0, 1);
  next.size = clamp(Number(next.size), MIN_SIZE, MAX_SIZE);
  next.opacity = clamp(Number(next.opacity), 0, 1);
  next.rotation = Number.isFinite(Number(next.rotation)) ? Number(next.rotation) : 0;
  if (next.path) next.path = tidyPath(next.path, clipMs);
  next.start_ms = clamp(Math.round(next.start_ms), 0, Math.max(0, clipMs - 100));
  if (next.end_ms != null) next.end_ms = clamp(Math.round(next.end_ms), next.start_ms + 100, Math.max(clipMs, next.start_ms + 100));
  return next;
}

/** What goes into an export request: only layers that start inside the clip, with ends kept to it. */
export function layersForRequest(layers, clipMs) {
  return (layers || [])
    .filter((l) => l.start_ms < clipMs)
    .map((l) => {
      const out = {
        kind: l.kind,
        start_ms: Math.max(0, Math.round(l.start_ms)),
        end_ms: l.end_ms == null || l.end_ms >= clipMs ? null : Math.round(l.end_ms),
        x: l.x,
        y: l.y,
        size: l.size,
        rotation: l.rotation,
        opacity: l.opacity,
      };
      const path = pathForRequest(l.path, clipMs);
      if (path.length) out.path = path;
      if (l.kind === "shape") {
        out.shape = l.shape;
        if (/^#[0-9a-fA-F]{6}$/.test(l.color || "")) out.color = l.color;
      } else {
        out.asset = l.asset;
      }
      return out;
    });
}

/** Whether the layer is on screen `relMs` into the clip. */
export function layerActive(layer, relMs) {
  return relMs >= layer.start_ms && (layer.end_ms == null || relMs < layer.end_ms);
}

/** The file to show for a layer: the server's own drawing of a shape, or the upload. */
export function layerSrc(layer) {
  if (layer.kind === "shape") {
    const q = /^#[0-9a-fA-F]{6}$/.test(layer.color || "") ? `?color=${encodeURIComponent(layer.color)}` : "";
    return withBase(`/api/sprites/${encodeURIComponent(layer.shape)}.png${q}`);
  }
  return withBase(`/api/overlays/${encodeURIComponent(layer.asset)}/file`);
}

/** Whether an uploaded layer is a video (rather than a picture). */
export function isVideoAsset(asset) {
  return /\.(mp4|webm)$/i.test(asset || "");
}

/** Where the layer's centre is `relMs` into the clip: on its path, or where it was put. */
export function layerPlace(layer, relMs) {
  return positionAt(layer.path, relMs, { x: layer.x, y: layer.y });
}

/**
 * The layer with its centre moved to `place` at `relMs`: a keyframe at that moment when it
 * follows a path, otherwise its fixed place.
 */
export function moveLayerTo(layer, place, relMs, clipMs) {
  if (layer.path && layer.path.length) {
    return { ...layer, path: setKeyframe(layer.path, relMs, place.x, place.y, clipMs) };
  }
  return { ...layer, x: place.x, y: place.y };
}

/** CSS placing the layer over a picture: centred on its spot (at `relMs`), its width a share of the picture's. */
export function layerStyle(layer, relMs = 0) {
  const at = layerPlace(layer, relMs);
  return {
    position: "absolute",
    left: `${at.x * 100}%`,
    top: `${at.y * 100}%`,
    width: `${layer.size * 100}%`,
    height: "auto",
    transform: `translate(-50%, -50%) rotate(${layer.rotation}deg)`,
    opacity: layer.opacity,
  };
}

/** The place (fractions of the picture) a pointer at `(clientX, clientY)` is over, kept inside it. */
export function pointToPlace(rect, clientX, clientY) {
  if (!rect || !(rect.width > 0) || !(rect.height > 0)) return null;
  return {
    x: clamp((clientX - rect.left) / rect.width, 0, 1),
    y: clamp((clientY - rect.top) / rect.height, 0, 1),
  };
}

/** A short name for a layer in the list. */
export function layerLabel(layer, overlays = [], shapes = FALLBACK_SHAPES) {
  if (layer.kind === "shape") return shapes.find((s) => s.id === layer.shape)?.label || layer.shape;
  const found = overlays.find((o) => o.id === layer.asset);
  return found ? found.name : layer.asset;
}

/** `1.5` for 1500 ms, as the time inputs show it. */
export function secondsText(ms) {
  return (Math.round(ms / 100) / 10).toString();
}
