import { useEffect, useMemo, useRef, useState } from "react";
import { api } from "./api.js";
import { gainAt, gainText, linePath, addPoint, movePoint, removePoint, MIN_GAP_MS } from "./volumeCurve.js";
import { secondsText } from "./layersModel.js";
import { LevelMeter } from "./LevelMeter.jsx";
import { MAX_VOLUME } from "./musicModel.js";
import { panText } from "./mixModel.js";

const W = 1000; // the lanes' drawing width; they stretch to the page
const STRIP_FRAMES = 12;
const MAX_STRIP_MS = 120_000;

// A song's loudness, kept for the session: picking a point shouldn't ask the server again.
const songPeaks = new Map();
function useSongPeaks(id) {
  const [wave, setWave] = useState(() => (id ? songPeaks.get(id) ?? null : null));
  useEffect(() => {
    if (!id) {
      setWave(null);
      return undefined;
    }
    if (songPeaks.has(id)) {
      setWave(songPeaks.get(id));
      return undefined;
    }
    let live = true;
    api
      .musicPeaks(id)
      .then((w) => {
        songPeaks.set(id, w);
        if (live) setWave(w);
      })
      .catch(() => live && setWave(null));
    return () => {
      live = false;
    };
  }, [id]);
  return wave;
}

/** The bars of a waveform over `[0, clipMs)`: `peaks` are laid from `fromMs` to `toMs`; the line shapes them. */
function barsPath(peaks, fromMs, toMs, clipMs, points) {
  if (!peaks || peaks.length === 0 || !(clipMs > 0) || toMs <= fromMs) return "";
  const x0 = (fromMs / clipMs) * W;
  const x1 = (toMs / clipMs) * W;
  const step = (x1 - x0) / peaks.length;
  return peaks
    .map((p, i) => {
      const x = x0 + (i + 0.5) * step;
      const t = ((i + 0.5) / peaks.length) * (toMs - fromMs) + fromMs;
      const h = Math.max(0.8, Math.sqrt(Math.min(1, Math.max(0, p))) * 46 * gainAt(points, t));
      return `M${x.toFixed(1)} ${(50 - h).toFixed(1)}V${(50 + h).toFixed(1)}`;
    })
    .join("");
}

/** The part of a song's peaks that plays: from `startMs` for `lenMs` of a `songMs` song. */
export function songBars(wave, startMs, lenMs) {
  if (!wave || !wave.peaks?.length || !(wave.duration_ms > 0)) return [];
  const n = wave.peaks.length;
  const a = Math.max(0, Math.floor((startMs / wave.duration_ms) * n));
  const b = Math.min(n, Math.max(a + 1, Math.ceil(((startMs + lenMs) / wave.duration_ms) * n)));
  return wave.peaks.slice(a, b);
}

/**
 * The mixing controls of one sound: mute, solo, its level, its pan and a meter of what is coming out.
 * `track` is `{muted, solo, level, pan, onMute, onSolo, onLevel, onPan, getEl, active}`; nothing is drawn
 * without one. Double-click the pan to put it back in the middle.
 */
function TrackControls({ name, track }) {
  if (!track) return null;
  return (
    <div className="mix-track">
      <div className="mix-buttons">
        <button
          type="button"
          className={"mix-btn" + (track.muted ? " on" : "")}
          aria-pressed={!!track.muted}
          aria-label={`Mute ${name}`}
          title={`Mute the ${name.toLowerCase()}`}
          onClick={track.onMute}
        >
          M
        </button>
        <button
          type="button"
          className={"mix-btn solo" + (track.solo ? " on" : "")}
          aria-pressed={!!track.solo}
          aria-label={`Solo ${name}`}
          title={`Hear only the ${name.toLowerCase()}`}
          onClick={track.onSolo}
        >
          S
        </button>
        <input
          type="range"
          min={0}
          max={MAX_VOLUME}
          step={0.05}
          value={track.level}
          aria-label={`${name} level`}
          title={`${name} level: ${Math.round(track.level * 100)}%`}
          onChange={(e) => track.onLevel(Number(e.target.value))}
        />
      </div>
      <label className="mix-pan" title={`${name} pan: ${panText(track.pan)} (double-click for the middle)`}>
        <span aria-hidden="true">L</span>
        <input
          type="range"
          min={-1}
          max={1}
          step={0.05}
          value={track.pan ?? 0}
          aria-label={`${name} pan`}
          aria-valuetext={panText(track.pan)}
          onChange={(e) => track.onPan(Number(e.target.value))}
          onDoubleClick={() => track.onPan(0)}
        />
        <span aria-hidden="true">R</span>
      </label>
      <LevelMeter getEl={track.getEl} active={!!track.active} label={name} />
    </div>
  );
}

/** The master: one fader for the whole mix and a meter as wide as the clip. `master` is `{level, onLevel, active}`. */
function MasterRow({ master }) {
  if (!master) return null;
  return (
    <div className="mix-row mix-master">
      <div className="mix-label">
        <b>Master</b>
        <input
          type="range"
          min={0}
          max={MAX_VOLUME}
          step={0.05}
          value={master.level}
          aria-label="Master level"
          title={`Master level: ${Math.round(master.level * 100)}%`}
          onChange={(e) => master.onLevel(Number(e.target.value))}
          onDoubleClick={() => master.onLevel(1)}
        />
      </div>
      <div className="mix-master-meter">
        <LevelMeter master active={!!master.active} label="Master" />
      </div>
    </div>
  );
}

/**
 * One lane that has a volume line: the sound's bars as they come out (the line applied), the line
 * itself and its points. Click on the lane to put a point on the line, drag a point to move it,
 * double-click it (or press Delete) to take it off; the arrow keys nudge a focused point.
 */
function CurveLane({ label, what, bars, clipMs, relMs, points, onChange, track, children }) {
  const lane = useRef(null);
  const drag = useRef(null);
  const where = (e) => {
    const r = lane.current.getBoundingClientRect();
    if (!(r.width > 0) || !(r.height > 0)) return null;
    return {
      t: Math.round(Math.min(1, Math.max(0, (e.clientX - r.left) / r.width)) * clipMs),
      gain: Math.min(1, Math.max(0, 1 - (e.clientY - r.top) / r.height)),
    };
  };
  const down = (e) => {
    if (e.button !== undefined && e.button !== 0) return;
    const at = where(e);
    if (!at) return;
    const idx = e.target?.dataset?.point;
    let i;
    if (idx !== undefined) {
      i = Number(idx);
    } else {
      const next = addPoint(points, at.t, at.gain, clipMs);
      onChange(next);
      i = next.findIndex((p) => Math.abs(p.t_ms - at.t) < MIN_GAP_MS);
    }
    drag.current = { i, points: null };
    lane.current.setPointerCapture?.(e.pointerId);
    e.preventDefault();
  };
  const move = (e) => {
    if (!drag.current || !(e.buttons & 1)) return;
    const at = where(e);
    if (!at) return;
    const base = drag.current.points || points;
    const next = movePoint(base, drag.current.i, at.t, at.gain, clipMs);
    drag.current.points = next;
    onChange(next);
  };
  const up = () => {
    drag.current = null;
  };
  const key = (e, i) => {
    const p = points[i];
    const big = e.shiftKey;
    if (e.key === "Delete" || e.key === "Backspace") onChange(removePoint(points, i));
    else if (e.key === "ArrowLeft") onChange(movePoint(points, i, p.t_ms - (big ? 1000 : 100), p.gain, clipMs));
    else if (e.key === "ArrowRight") onChange(movePoint(points, i, p.t_ms + (big ? 1000 : 100), p.gain, clipMs));
    else if (e.key === "ArrowUp") onChange(movePoint(points, i, p.t_ms, p.gain + (big ? 0.2 : 0.05), clipMs));
    else if (e.key === "ArrowDown") onChange(movePoint(points, i, p.t_ms, p.gain - (big ? 0.2 : 0.05), clipMs));
    else return;
    e.preventDefault();
  };
  const pct = clipMs > 0 ? Math.min(100, (relMs / clipMs) * 100) : 0;
  const quiet = points.length > 0;
  return (
    <div className="mix-row">
      <div className="mix-label">
        <b>{label}</b>
        {quiet && (
          <button type="button" className="link" onClick={() => onChange([])} aria-label={`Reset the ${what} line`}>
            reset
          </button>
        )}
        <TrackControls name={label} track={track} />
      </div>
      <div
        ref={lane}
        className="mix-lane mix-curve"
        role="group"
        aria-label={`${what} over the clip`}
        onPointerDown={down}
        onPointerMove={move}
        onPointerUp={up}
        onPointerCancel={up}
      >
        <svg viewBox={`0 0 ${W} 100`} preserveAspectRatio="none" aria-hidden="true">
          <path className="mix-bars" d={bars} />
          <path className="mix-line" d={linePath(points, clipMs, W, 100)} />
        </svg>
        {points.map((p, i) => (
          <button
            key={i}
            type="button"
            data-point={i}
            className="mix-point"
            style={{ left: `${(p.t_ms / clipMs) * 100}%`, top: `${(1 - p.gain) * 100}%` }}
            aria-label={`${what} at ${secondsText(p.t_ms)} s: ${gainText(p.gain)}`}
            onKeyDown={(e) => key(e, i)}
            onDoubleClick={() => onChange(removePoint(points, i))}
          />
        ))}
        <div className="mix-playhead" style={{ left: `${pct}%` }} />
        {children}
      </div>
    </div>
  );
}

/**
 * The picture and the sound side by side over the clip, so the volume can be shaped against what is
 * on screen: the filmstrip, the clip's own sound and the song, each as wide as the clip is long, with
 * the playhead across all of them. The sound lanes carry a volume line you edit by clicking and dragging,
 * and (given `clipTrack` / `songTrack`) mute, solo, a level, a pan and a meter; `master` adds the fader and
 * the meter of the whole mix.
 */
export function MixTimeline({
  ratingKey,
  start,
  end,
  relMs,
  onSeek,
  audio,
  clipCurve,
  onClipCurve,
  music,
  onMusicCurve,
  clipSound = true,
  clipTrack,
  songTrack,
  master,
}) {
  const clipMs = Math.max(0, end - start);
  const [own, setOwn] = useState(null); // {has_audio, peaks} of the clip's own sound
  // The waveform follows the clip a moment after it stops moving.
  useEffect(() => {
    setOwn(null);
    if (!audio || !clipSound || !(clipMs > 0)) return undefined;
    let live = true;
    const timer = setTimeout(() => {
      api
        .clipWaveform(ratingKey, start, end)
        .then((w) => live && setOwn(w))
        .catch(() => live && setOwn({ has_audio: true, peaks: [] }));
    }, 400);
    return () => {
      live = false;
      clearTimeout(timer);
    };
  }, [ratingKey, start, end, audio, clipSound, clipMs]);

  const song = useSongPeaks(music?.id);
  const musicLen = music ? Math.max(0, clipMs - music.at_ms) : 0;
  const songPart = useMemo(() => (music ? songBars(song, music.start_ms, musicLen) : []), [song, music?.start_ms, musicLen, music]);

  if (!(clipMs > 0)) return null;
  const stripUrl = clipMs <= MAX_STRIP_MS ? api.filmstripUrl(ratingKey, start, end, STRIP_FRAMES, 44) : null;
  const pct = Math.min(100, (relMs / clipMs) * 100);
  const seek = (e) => {
    const r = e.currentTarget.getBoundingClientRect();
    if (r.width > 0) onSeek?.(start + Math.round(Math.min(1, Math.max(0, (e.clientX - r.left) / r.width)) * clipMs));
  };

  return (
    <section className="mix-timeline" aria-label="Sound over the clip">
      <div className="mix-head">
        <b>Mix</b>
        <span className="muted">
          Click a sound lane to add a point to its volume line, drag to move it, double-click a point to remove it.
        </span>
      </div>
      <div className="mix-row">
        <div className="mix-label">
          <b>Picture</b>
        </div>
        <div
          className="mix-lane mix-video"
          style={stripUrl ? { backgroundImage: `url("${stripUrl}")` } : undefined}
          onPointerDown={seek}
          role="presentation"
        >
          <div className="mix-playhead" style={{ left: `${pct}%` }} />
        </div>
      </div>
      {audio && clipSound && (
        <CurveLane
          label="Clip sound"
          what="Clip sound volume"
          clipMs={clipMs}
          relMs={relMs}
          points={clipCurve}
          onChange={onClipCurve}
          track={clipTrack}
          bars={barsPath(own?.peaks, 0, clipMs, clipMs, clipCurve)}
        >
          {own && !own.has_audio && <span className="mix-note">This video has no sound.</span>}
        </CurveLane>
      )}
      {music && (
        <CurveLane
          label="Music"
          what="Music volume"
          clipMs={clipMs}
          relMs={relMs}
          points={music.curve || []}
          onChange={onMusicCurve}
          track={songTrack}
          bars={barsPath(songPart, music.at_ms, clipMs, clipMs, music.curve || [])}
        >
          {music.at_ms > 0 && <span className="mix-come-in" style={{ left: `${(music.at_ms / clipMs) * 100}%` }} title="The song comes in here" />}
        </CurveLane>
      )}
      <MasterRow master={master} />
      <div className="mix-scale muted" aria-hidden="true">
        <span>0</span>
        <span>{secondsText(clipMs)} s</span>
      </div>
    </section>
  );
}
