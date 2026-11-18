import { useEffect, useRef, useState } from "react";
import { api } from "./api.js";
import {
  MAX_VOLUME,
  adjustMusic,
  durationText,
  musicSrc,
  newMusic,
  percentText,
  previewState,
  songTimeText,
} from "./musicModel.js";
import { secondsText } from "./layersModel.js";
import { Waveform } from "./Waveform.jsx";

/** One slider with its value shown. */
function Slider({ label, value, min, max, step, text, onChange, disabled }) {
  return (
    <label>
      {label} <b>{text(value)}</b>
      <input
        type="range"
        min={min}
        max={max}
        step={step}
        value={value}
        disabled={disabled}
        onChange={(e) => onChange(Number(e.target.value))}
      />
    </label>
  );
}

/**
 * The Music box: a song from Navidrome under the clip, with the volume of the song and of the
 * clip's own sound, where in the song to start, when it comes in, and its fades. The song plays
 * in the page alongside the player (up to full volume; the export can go louder).
 * `relMs` is the playhead in clip time and `clipMs` the clip's length.
 */
export function MusicPanel({ music, onChoose, onClear, onChange, clipVolume, setClipVolume, audio, clipMs, relMs, playing, preview = true, what = "clip" }) {
  const [configured, setConfigured] = useState(null); // null: not known yet
  const [query, setQuery] = useState("");
  const [results, setResults] = useState(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(null);
  const [hear, setHear] = useState(true);
  const player = useRef(null);
  const [wave, setWave] = useState(null); // {duration_ms, peaks} of the chosen song
  const [waveError, setWaveError] = useState(null);

  useEffect(() => {
    let live = true;
    api
      .getNavidromeSettings()
      .then((s) => live && setConfigured(!!s.configured))
      .catch(() => live && setConfigured(false));
    return () => {
      live = false;
    };
  }, []);

  // The chosen song's waveform, for picking where it starts.
  const musicId = music?.id;
  useEffect(() => {
    setWave(null);
    setWaveError(null);
    if (!musicId) return undefined;
    let live = true;
    api
      .musicPeaks(musicId)
      .then((w) => live && setWave(w))
      .catch((err) => live && setWaveError(err.message));
    return () => {
      live = false;
    };
  }, [musicId]);

  async function search(e) {
    e?.preventDefault();
    const q = query.trim();
    if (!q) return;
    setBusy(true);
    setError(null);
    try {
      const r = await api.musicSearch(q);
      setResults(r.tracks || []);
    } catch (err) {
      setResults(null);
      setError(err.message);
    } finally {
      setBusy(false);
    }
  }

  // The song in the page follows the player: on when the clip reaches it, at the right place in
  // the song, at the right volume (fades included).
  const state = previewState(music, relMs, clipMs);
  useEffect(() => {
    if (player.current) player.current.volume = hear ? state.gain : 0;
  }, [state.gain, hear, music?.id]);
  useEffect(() => {
    const a = player.current;
    if (!a || !music) return;
    if (!playing || !state.on || !hear) {
      a.pause();
      return;
    }
    const want = state.songMs / 1000;
    if (a.paused || Math.abs(a.currentTime - want) > 0.35) {
      try {
        a.currentTime = want;
      } catch (_) {
        // not loaded yet: it starts from the beginning of what it has
      }
    }
    if (a.paused) a.play().catch(() => {});
  }, [playing, state.on, state.songMs, hear, music?.id]);

  const set = (changes) => onChange(adjustMusic(music, changes, clipMs));
  const playingMs = music ? Math.max(0, clipMs - music.at_ms) : 0;
  const songMs = wave?.duration_ms > 0 ? wave.duration_ms : music?.duration_s > 0 ? Math.round(music.duration_s * 1000) : 600_000;

  return (
    <fieldset className="layers-panel music-panel">
      <legend>Music</legend>

      {configured === false && (
        <p className="hint tiny">
          Connect Navidrome under <b>Settings</b> to put one of your songs under the {what}.
        </p>
      )}

      {configured && !music && (
        <>
          <form className="layer-add" onSubmit={search}>
            <input
              type="search"
              aria-label="Search your music"
              placeholder="Search your music"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
            />
            <button type="submit" className="small secondary" disabled={busy || !query.trim()}>
              {busy ? "Searching…" : "Search"}
            </button>
          </form>
          {error && (
            <p className="error" role="alert">
              {error}
            </p>
          )}
          {results && results.length === 0 && <p className="hint tiny">No songs found.</p>}
          {results && results.length > 0 && (
            <ul className="music-results">
              {results.map((t) => (
                <li key={t.id}>
                  <button type="button" className="link" onClick={() => onChoose(newMusic(t))}>
                    <b>{t.title || "Untitled"}</b>
                    {t.artist ? ` · ${t.artist}` : ""}
                    {t.duration_s ? ` · ${durationText(t.duration_s)}` : ""}
                  </button>
                </li>
              ))}
            </ul>
          )}
        </>
      )}

      {music && (
        <div className="layer-controls">
          <p className="music-title">
            ♪ <b>{music.title || "Untitled"}</b>
            {music.artist ? ` · ${music.artist}` : ""}
            {music.duration_s ? ` · ${durationText(music.duration_s)}` : ""}{" "}
            <button type="button" className="link" onClick={onClear}>
              Remove
            </button>
          </p>
          <Slider
            label="Music volume"
            value={music.volume}
            min={0}
            max={MAX_VOLUME}
            step={0.05}
            text={percentText}
            onChange={(v) => set({ volume: v })}
          />
          <Waveform
            peaks={wave?.peaks}
            songMs={songMs}
            startMs={music.start_ms}
            playingMs={playingMs}
            onPick={(v) => set({ start_ms: v })}
          />
          {waveError && <p className="hint tiny">Couldn't draw the waveform ({waveError}); use the slider.</p>}
          <Slider
            label="Start the song at"
            value={music.start_ms}
            min={0}
            max={Math.max(0, songMs - 1000)}
            step={500}
            text={songTimeText}
            onChange={(v) => set({ start_ms: v })}
          />
          <Slider
            label="Comes in at"
            value={music.at_ms}
            min={0}
            max={Math.max(0, clipMs - 100)}
            step={100}
            text={(v) => `${secondsText(v)} s`}
            onChange={(v) => set({ at_ms: v })}
          />
          <Slider
            label="Fade in"
            value={music.fade_in_ms}
            min={0}
            max={playingMs}
            step={100}
            text={(v) => `${secondsText(v)} s`}
            onChange={(v) => set({ fade_in_ms: v })}
          />
          <Slider
            label="Fade out"
            value={music.fade_out_ms}
            min={0}
            max={playingMs}
            step={100}
            text={(v) => `${secondsText(v)} s`}
            onChange={(v) => set({ fade_out_ms: v })}
          />
          {preview && (
            <>
              <label className="check">
                <input type="checkbox" checked={hear} onChange={(e) => setHear(e.target.checked)} /> Hear it in the player
              </label>
              <audio ref={player} src={musicSrc(music.id)} preload="auto" loop={false} />
            </>
          )}
        </div>
      )}

      <div className="layer-controls">
        <Slider
          label={what === "clip" ? "Clip sound volume" : "Clips' sound volume"}
          value={clipVolume}
          min={0}
          max={MAX_VOLUME}
          step={0.05}
          text={percentText}
          disabled={!audio}
          onChange={setClipVolume}
        />
        {!audio && (
          <p className="hint tiny">
            Tick <b>Include audio</b> (MP4 or WebM) to keep the {what === "clip" ? "clip's" : "clips'"} own sound and set its volume.
          </p>
        )}
      </div>
      {music && (
        <p className="hint tiny">
          Music is in MP4 and WebM exports only.
          {preview ? " The player plays it up to 100%; the export goes up to 200%." : " The export goes up to 200%; it is not played here."} The song
          stops where the {what} does.
        </p>
      )}
    </fieldset>
  );
}
