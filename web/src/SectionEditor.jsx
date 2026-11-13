import { useEffect, useState } from "react";
import { TrimPanel } from "./TrimPanel.jsx";
import { api } from "./api.js";
import { SCALE_DEFAULT, SCALE_MAX, SCALE_MIN, SCALE_STEP, fontOptions } from "./fonts.js";
import { episodeLabel, fmtTime } from "./format.js";
import { trim } from "./sequenceModel.js";
import { fpsOf } from "./timecode.js";

/** The caption a section has after switching to `mode`, keeping what carries over. */
export function captionFor(clip, mode) {
  const cur = clip.caption || { mode: "none" };
  if (mode === cur.mode) return cur;
  const font = cur.font ?? null;
  const font_scale = cur.font_scale ?? SCALE_DEFAULT;
  if (mode === "none") return { mode: "none" };
  if (mode === "source") return { mode: "source", cues: null, font, font_scale };
  return { mode: "meme", top: null, bottom: clip.line || null, font_size: 36, font, font_scale };
}

/**
 * Edit one section of the sequence: its exact In/Out, its captions and font,
 * and (for burned subtitles) the sync. Everything lands in the sequence at once.
 * "Detailed edit" opens the full editor for the lines and everything else.
 */
export function SectionEditor({ clip, index, count, onChange, onDetailed }) {
  const [fonts, setFonts] = useState({ fonts: [], default: null });
  const [info, setInfo] = useState(null); // video-info, for frame numbers
  const [lock, setLock] = useState(false);
  const [snap, setSnap] = useState("frame");

  useEffect(() => {
    let alive = true;
    api
      .fonts()
      .then((d) => alive && setFonts(d))
      .catch(() => {});
    return () => {
      alive = false;
    };
  }, []);

  useEffect(() => {
    let alive = true;
    setInfo(null);
    api
      .videoInfo(clip.rating_key)
      .then((v) => alive && setInfo(v))
      .catch(() => {});
    return () => {
      alive = false;
    };
  }, [clip.rating_key]);

  const cap = clip.caption || { mode: "none" };
  const setCaption = (c) => onChange({ ...clip, caption: c, subtitle_offset_ms: c.mode === "source" ? clip.subtitle_offset_ms || 0 : 0 });
  const offset = clip.subtitle_offset_ms || 0;

  return (
    <div className="panel section-editor">
      <h3>
        Section {index + 1} of {count}
        <span className="hint tiny"> · {episodeLabel(clip)}</span>
      </h3>
      {clip.line && <p className="seq-line">“{clip.line}”</p>}

      <TrimPanel
        start={clip.start_ms}
        end={clip.end_ms}
        duration={0}
        fps={fpsOf(info)}
        playhead={null}
        lockLength={lock}
        onLock={setLock}
        snap={snap}
        onSnap={setSnap}
        showKeys={false}
        onTrim={(s, e, _commit, moved) => onChange(trimClip(clip, s, e, moved))}
      />

      <fieldset className="section-captions">
        <legend>Captions</legend>
        {[
          ["none", "None"],
          ["source", "Burn subtitles"],
          ["meme", "Meme text"],
        ].map(([mode, label]) => (
          <label key={mode}>
            <input type="radio" checked={cap.mode === mode} onChange={() => setCaption(captionFor(clip, mode))} />
            {label}
          </label>
        ))}
        {cap.mode === "meme" && (
          <div className="meme-inputs">
            <input
              placeholder="Top text"
              value={cap.top || ""}
              onChange={(e) => setCaption({ ...cap, top: e.target.value || null })}
            />
            <input
              placeholder="Bottom text"
              value={cap.bottom || ""}
              onChange={(e) => setCaption({ ...cap, bottom: e.target.value || null })}
            />
          </div>
        )}
        {cap.mode !== "none" && (
          <div className="font-controls">
            <label>
              Font
              <select value={cap.font || ""} onChange={(e) => setCaption({ ...cap, font: e.target.value || null })}>
                {fontOptions(fonts.fonts, fonts.default).map((o) => (
                  <option key={o.id} value={o.id}>
                    {o.label}
                  </option>
                ))}
              </select>
            </label>
            <label>
              Text size <b>{cap.font_scale || SCALE_DEFAULT}%</b>
              <input
                type="range"
                min={SCALE_MIN}
                max={SCALE_MAX}
                step={SCALE_STEP}
                value={cap.font_scale || SCALE_DEFAULT}
                onChange={(e) => setCaption({ ...cap, font_scale: Number(e.target.value) })}
              />
            </label>
          </div>
        )}
        {cap.mode === "source" && (
          <div className="sync-control">
            <div className="sync-value">
              Subtitle sync{" "}
              <b>
                {offset > 0 ? "+" : ""}
                {offset}ms
              </b>
              {offset !== 0 && (
                <button type="button" className="link" onClick={() => onChange({ ...clip, subtitle_offset_ms: 0 })}>
                  reset
                </button>
              )}
            </div>
            <div className="nudge">
              <span>Sync</span>
              {[-500, -100, 100, 500].map((d) => (
                <button key={d} type="button" onClick={() => onChange({ ...clip, subtitle_offset_ms: offset + d })}>
                  {d > 0 ? `+${d}` : d}
                </button>
              ))}
            </div>
            {Array.isArray(cap.cues) && <p className="hint tiny">This section has subtitle lines edited in the full editor.</p>}
          </div>
        )}
      </fieldset>

      <div className="btn-row">
        <button type="button" className="secondary" onClick={onDetailed} title="Open this section in the full editor: player, timeline, every subtitle line">
          ✎ Detailed edit
        </button>
        <span className="hint tiny">
          {fmtTime(clip.start_ms)} → {fmtTime(clip.end_ms)}
        </span>
      </div>
    </div>
  );
}

/** `clip` with a new trim (kept sensible). */
function trimClip(clip, start, end, moved) {
  return trim([clip], clip.id, start, end, moved)[0];
}
