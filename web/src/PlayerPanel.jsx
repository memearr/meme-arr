import { fmtTime } from "./format.js";
import { LayerOverlay } from "./LayerOverlay.jsx";

const SPEEDS = [0.25, 0.5, 1];

/** The preview video with the caption drawn over it, and the transport bar under it. */
export function PlayerPanel({
  previewError,
  setPreviewError,
  proxy,
  proxyLoading,
  previewStalled,
  videoRef,
  measurePicture,
  proxyLoaded,
  playing,
  setPlaying,
  setPlayhead,
  mediaTimeNow,
  capStyle,
  captionMode,
  overlayText,
  lookView,
  pictureBox,
  layers,
  relMs,
  selectedLayer,
  setSelectedLayer,
  moveLayer,
  top,
  bottom,
  playhead,
  srcFps,
  seekTo,
  start,
  end,
  stepPlayhead,
  togglePlay,
  speed,
  setSpeed,
  volume,
  setVolume,
}) {
  return (
      <div className="player">
        <div className="preview">
          {previewError && (
            <div className="placeholder preview-error">
              <div>⚠ Preview failed</div>
              <p className="error">{previewError}</p>
            </div>
          )}
          {!previewError && proxy && (
            <div className="player-stage">
              <video
                key={proxy.src}
                ref={videoRef}
                src={proxy.src}
                playsInline
                onLoadedMetadata={measurePicture}
                onLoadedData={proxyLoaded}
                onPlay={() => setPlaying(true)}
                onPause={() => setPlaying(false)}
                onSeeked={() => {
                  if (!playing) {
                    const now = mediaTimeNow();
                    if (now != null) setPlayhead(now);
                  }
                }}
                onError={() =>
                  setPreviewError(
                    "The preview file was generated but this browser couldn't decode it " +
                      "(likely an unusual pixel format or codec quirk in the source). " +
                      "Exporting may still work — try a smaller width or a different format."
                  )
                }
              />
              {capStyle && (
                <div className="cap-layer" style={capStyle.layer}>
                  {captionMode === "source" && !lookView && overlayText && (
                    <div className="cap-overlay cap-bottom" style={capStyle.bottom}>
                      {overlayText}
                    </div>
                  )}
                  {captionMode === "source" && lookView && (
                    <div className="cap-look" style={lookView.style}>
                      {lookView.lines.map((words, li) => (
                        <div key={li}>
                          {words.map((w, wi) => (
                            <span
                              key={wi}
                              style={{
                                visibility: w.shown ? "visible" : "hidden",
                                fontSize: w.emphasis ? `${lookView.emphasis}em` : undefined,
                              }}
                            >
                              {w.text}
                              {wi + 1 < words.length ? " " : ""}
                            </span>
                          ))}
                        </div>
                      ))}
                    </div>
                  )}
                  {captionMode === "meme" && top && (
                    <div className="cap-overlay cap-top" style={capStyle.top}>
                      {top}
                    </div>
                  )}
                  {captionMode === "meme" && bottom && (
                    <div className="cap-overlay cap-bottom" style={capStyle.bottom}>
                      {bottom}
                    </div>
                  )}
                </div>
              )}
              <LayerOverlay
                layers={layers}
                relMs={relMs}
                pictureBox={pictureBox}
                selectedId={selectedLayer}
                onSelect={setSelectedLayer}
                onMove={moveLayer}
              />
              <span className="player-chip mono">
                {fmtTime(playhead)}
                {srcFps ? ` · f${Math.round((playhead * srcFps.num) / (1000 * srcFps.den))}` : ""}
              </span>
            </div>
          )}
          {!previewError && !proxy && (
            <div className="placeholder">{proxyLoading ? "Preparing the preview…" : "Preview will appear here"}</div>
          )}
          {previewStalled && !previewError && (
            <p className="hint preview-stalled" role="status">
              The preview hasn't started. Move the clip a little to render it again; if it keeps happening,
              the server log (System → Logs) says why.
            </p>
          )}
        </div>

        <div className="transport">
          <button type="button" className="tp" onClick={() => seekTo(start)} aria-label="Go to In point" title="Go to In">
            ⏮
          </button>
          <button type="button" className="tp" onClick={() => stepPlayhead(-1)} aria-label="Back one frame" title="Back one frame (,)">
            ◀
          </button>
          <button
            type="button"
            className="tp hot"
            onClick={togglePlay}
            aria-label={playing ? "Pause" : "Play the clip"}
            title="Play / pause (Space)"
            disabled={!proxy}
          >
            {playing ? "⏸" : "▶"}
          </button>
          <button type="button" className="tp" onClick={() => stepPlayhead(1)} aria-label="Forward one frame" title="Forward one frame (.)">
            ▶
          </button>
          <button type="button" className="tp" onClick={() => seekTo(Math.max(start, end - 1))} aria-label="Go to Out point" title="Go to Out">
            ⏭
          </button>
          <label className="tp-speed">
            Speed
            <select value={speed} onChange={(e) => setSpeed(Number(e.target.value))}>
              {SPEEDS.map((s) => (
                <option key={s} value={s}>
                  {s}×
                </option>
              ))}
            </select>
          </label>
          <div className="tp-volume">
            <button
              type="button"
              className="tp"
              onClick={() => setVolume((v) => ({ ...v, muted: !v.muted }))}
              aria-label={volume.muted || volume.level === 0 ? "Turn sound on" : "Mute"}
              aria-pressed={volume.muted}
              title={volume.muted ? "Sound is off (click to turn on)" : "Sound is on (click to mute)"}
            >
              {volume.muted || volume.level === 0 ? "🔇" : volume.level < 0.4 ? "🔈" : "🔊"}
            </button>
            <input
              type="range"
              min="0"
              max="1"
              step="0.05"
              value={volume.muted ? 0 : volume.level}
              aria-label="Volume"
              onChange={(e) => {
                const level = Number(e.target.value);
                setVolume({ level, muted: level === 0 });
              }}
            />
          </div>
          <span className="tp-spacer" />
          <span className={"player-status" + (proxyLoading ? " busy" : "")} role="status">
            {proxyLoading ? "Updating preview…" : "Preview in sync"}
          </span>
        </div>
      </div>
  );
}
