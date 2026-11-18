import { songWindow, startAtFraction, songTimeText } from "./musicModel.js";

/** Tall bars for loud parts, short for quiet; one path so hundreds of bars stay cheap. */
function barsPath(peaks) {
  return peaks
    .map((p, i) => {
      const h = Math.max(1, Math.sqrt(Math.min(1, Math.max(0, p))) * 48);
      return `M${i + 0.5} ${50 - h}V${50 + h}`;
    })
    .join("");
}

/**
 * A song's waveform with the stretch that will play highlighted. Click or drag to put that stretch
 * where the song is at its best; the arrow keys nudge it a second at a time.
 * `songMs` is the song's length, `startMs` where playing starts, `playingMs` how long it plays.
 */
export function Waveform({ peaks, songMs, startMs, playingMs, onPick }) {
  if (!peaks || peaks.length === 0 || !(songMs > 0)) return null;
  const n = peaks.length;
  const win = songWindow(startMs, playingMs, songMs);

  const pick = (e) => {
    const r = e.currentTarget.getBoundingClientRect();
    if (!(r.width > 0)) return;
    onPick(startAtFraction((e.clientX - r.left) / r.width, songMs, playingMs));
  };
  const nudge = (e) => {
    const step = e.shiftKey ? 5000 : 1000;
    if (e.key === "ArrowLeft") onPick(startAtFraction((startMs - step + playingMs / 2) / songMs, songMs, playingMs));
    else if (e.key === "ArrowRight") onPick(startAtFraction((startMs + step + playingMs / 2) / songMs, songMs, playingMs));
    else return;
    e.preventDefault();
  };

  return (
    <svg
      className="waveform"
      viewBox={`0 0 ${n} 100`}
      preserveAspectRatio="none"
      role="slider"
      tabIndex={0}
      aria-label="Where in the song to start"
      aria-valuemin={0}
      aria-valuemax={Math.round(songMs)}
      aria-valuenow={Math.round(startMs)}
      aria-valuetext={songTimeText(startMs)}
      onPointerDown={(e) => {
        e.currentTarget.setPointerCapture?.(e.pointerId);
        pick(e);
      }}
      onPointerMove={(e) => {
        if (e.buttons & 1) pick(e);
      }}
      onKeyDown={nudge}
    >
      <rect className="wave-window" x={win.left * n} y="0" width={win.width * n} height="100" />
      <path className="wave-bars" d={barsPath(peaks)} />
    </svg>
  );
}
