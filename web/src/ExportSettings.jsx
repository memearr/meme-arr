import { AUDIO_FORMATS } from "./clipSettings.js";
import { fpsValue } from "./timecode.js";

/** The Format and Output boxes: container, audio, the source credit, size, frame rate and quality. */
export function ExportSettings({
  format,
  setFormat,
  audio,
  toggleAudio,
  creditSource,
  setCreditSource,
  width,
  setWidth,
  fps,
  setFps,
  quality,
  setQuality,
  srcFps,
  clipLength,
  outFrames,
}) {
  return (
    <>
      <fieldset>
        <legend>Format</legend>
        {["webp", "mp4", "webm", "gif"].map((f) => {
          const disabled = audio && !AUDIO_FORMATS.includes(f);
          return (
            <label key={f} className={disabled ? "disabled" : undefined}>
              <input
                type="radio"
                checked={format === f}
                disabled={disabled}
                onChange={() => setFormat(f)}
              />
              {f.toUpperCase()}
            </label>
          );
        })}
        <label className="audio-toggle">
          <input type="checkbox" checked={audio} onChange={(e) => toggleAudio(e.target.checked)} />
          Include audio (Opus)
        </label>
        {audio && <p className="hint tiny">GIF and WebP can't carry audio — MP4/WebM only while this is on.</p>}
        <label
          className="audio-toggle"
          title="Everything else from the original file (its name, tags, chapters, subtitle tracks) is always removed from the export."
        >
          <input type="checkbox" checked={creditSource} onChange={(e) => setCreditSource(e.target.checked)} />
          Name the source in the file's title (MP4/WebM)
        </label>
      </fieldset>

      <fieldset>
        <legend>Output</legend>
        <label className="slider">
          Width {width}px
          <input type="range" min="240" max="960" step="40" value={width} onChange={(e) => setWidth(+e.target.value)} />
        </label>
        <label className="slider">
          FPS {fps}
          <input type="range" min="8" max="30" value={fps} onChange={(e) => setFps(+e.target.value)} />
        </label>
        {srcFps && (
          <button
            type="button"
            className="link"
            onClick={() => setFps(Math.min(30, Math.max(8, Math.round(fpsValue(srcFps)))))}
            title="Use the video's own frame rate, so every source frame is kept"
          >
            match source ({fpsValue(srcFps).toFixed(3)} fps)
          </button>
        )}
        <label className="slider">
          Quality {quality}
          <input type="range" min="10" max="100" value={quality} onChange={(e) => setQuality(+e.target.value)} />
        </label>
        <p className="hint tiny output-note">
          ≈ {outFrames} frame{outFrames === 1 ? "" : "s"} at {fps} fps · {(clipLength / 1000).toFixed(2)} s
        </p>
      </fieldset>
    </>
  );
}
