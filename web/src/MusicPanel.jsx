import { useEffect, useState } from "react";
import { api } from "./api.js";
import {
  MAX_VOLUME,
  adjustMusic,
  coverSrc,
  durationText,
  newMusic,
  percentText,
  songTimeText,
} from "./musicModel.js";
import { secondsText } from "./layersModel.js";
import { SongPreview } from "./SongPreview.jsx";
import { Waveform } from "./Waveform.jsx";
import { browserStorage } from "./editorStorage.js";
import { clearRecent, forgetSong, loadRecent, rememberSong } from "./recentSongs.js";

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

/** One song in a list: its cover, title, artist and length; a click picks it. */
const SOURCE_LABEL = { upload: " · uploaded", library: " · library", jellyfin: " · Jellyfin" };

function SongRow({ track, onPick, onRemove }) {
  return (
    <li className={onRemove ? "song-item removable" : undefined}>
      <button type="button" className="link song-row" onClick={() => onPick(track)}>
        {track.cover_art ? (
          <img className="song-art" src={coverSrc(track.cover_art, 64)} alt="" width="32" height="32" loading="lazy" />
        ) : (
          <span className="song-art none" aria-hidden="true">
            ♪
          </span>
        )}
        <span>
          <b>{track.title || "Untitled"}</b>
          {track.artist ? ` · ${track.artist}` : ""}
          {track.duration_s ? ` · ${durationText(track.duration_s)}` : ""}
          {SOURCE_LABEL[track.source] || ""}
        </span>
      </button>
      {onRemove && (
        <button type="button" className="link" aria-label={`Remove ${track.title || "this song"}`} onClick={() => onRemove(track)}>
          remove
        </button>
      )}
    </li>
  );
}

/**
 * The Music box: a song under the clip (from Navidrome, Jellyfin, a folder of music on the server, or uploaded), with the volume of the song and of the
 * clip's own sound, where in the song to start, when it comes in, and its fades. The song plays
 * in the page alongside the player (up to full volume; the export can go louder).
 * `relMs` is the playhead in clip time and `clipMs` the clip's length.
 */
export function MusicPanel({ music, onChoose, onClear, onChange, clipVolume, setClipVolume, audio, clipMs, relMs, playing, preview = true, what = "clip", showClipVolume = true }) {
  const [sources, setSources] = useState(null); // {navidrome, library, uploads} once known
  const [uploads, setUploads] = useState([]); // the songs uploaded to the app
  const [uploading, setUploading] = useState(false);
  const [warning, setWarning] = useState(null); // a source that couldn't be searched
  const [query, setQuery] = useState("");
  const [results, setResults] = useState(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(null);
  const [hear, setHear] = useState(true);
  const [recent, setRecent] = useState(() => loadRecent(browserStorage())); // songs picked lately
  const [wave, setWave] = useState(null); // {duration_ms, peaks} of the chosen song
  const [waveError, setWaveError] = useState(null);

  const refreshSources = () =>
    api
      .musicSources()
      .then(setSources)
      .catch(() => setSources((s) => s || { navidrome: false, library: false, uploads: 0 }));
  useEffect(() => {
    let live = true;
    api
      .musicSources()
      .then((s) => live && setSources(s))
      .catch(() => live && setSources({ navidrome: false, library: false, uploads: 0 }));
    return () => {
      live = false;
    };
  }, []);

  // The uploaded songs, listed before a search so they are easy to find again.
  const uploadCount = sources?.uploads || 0;
  useEffect(() => {
    if (!uploadCount) {
      setUploads([]);
      return undefined;
    }
    let live = true;
    api
      .songUploads()
      .then((r) => live && setUploads(r.tracks || []))
      .catch(() => {});
    return () => {
      live = false;
    };
  }, [uploadCount]);

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

  // Picking a song also puts it first among the recent ones.
  function pick(track) {
    setRecent((list) => rememberSong(browserStorage(), list, track));
    onChoose(newMusic(track));
  }

  async function upload(file) {
    if (!file) return;
    setUploading(true);
    setError(null);
    try {
      const r = await api.uploadSong(file);
      await refreshSources();
      pick(r.track);
    } catch (err) {
      setError(err.message);
    } finally {
      setUploading(false);
    }
  }

  async function removeUpload(track) {
    setError(null);
    try {
      await api.deleteSongUpload(track.id);
      setRecent((list) => forgetSong(browserStorage(), list, track.id));
      setResults((r) => (r ? r.filter((t) => t.id !== track.id) : r));
      await refreshSources();
    } catch (err) {
      setError(err.message);
    }
  }

  async function search(e) {
    e?.preventDefault();
    const q = query.trim();
    if (!q) return;
    setBusy(true);
    setError(null);
    try {
      const r = await api.musicSearch(q);
      setResults(r.tracks || []);
      setWarning(r.warning || null);
    } catch (err) {
      setResults(null);
      setError(err.message);
    } finally {
      setBusy(false);
    }
  }

  const set = (changes) => onChange(adjustMusic(music, changes, clipMs));
  const playingMs = music ? Math.max(0, clipMs - music.at_ms) : 0;
  const songMs = wave?.duration_ms > 0 ? wave.duration_ms : music?.duration_s > 0 ? Math.round(music.duration_s * 1000) : 600_000;

  return (
    <fieldset className="layers-panel music-panel">
      <legend>Music</legend>

      {sources && !sources.navidrome && !sources.library && !sources.jellyfin && (
        <p className="hint tiny">
          Connect Navidrome under <b>Settings</b> to search your music for a song to put under the {what}, or
          upload a song here.
        </p>
      )}

      {sources && !music && (
        <>
          {(sources.navidrome || sources.library || sources.jellyfin || uploadCount > 0) && (
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
          )}
          <div className="layer-add">
            <label className="file-btn">
              {uploading ? "Uploading…" : "Upload a song…"}
              <input
                type="file"
                accept="audio/*,.mp3,.flac,.ogg,.opus,.m4a,.wav,.webm"
                disabled={uploading}
                onChange={(e) => {
                  upload(e.target.files?.[0]);
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
          {warning && <p className="hint tiny">{warning}</p>}
          {results && results.length === 0 && <p className="hint tiny">No songs found.</p>}
          {results && results.length > 0 && (
            <ul className="music-results">
              {results.map((t) => (
                <SongRow key={t.id} track={t} onPick={pick} />
              ))}
            </ul>
          )}
          {!results && recent.length > 0 && (
            <>
              <p className="hint tiny music-recent-head">
                Recent songs{" "}
                <button type="button" className="link" onClick={() => setRecent(clearRecent(browserStorage()))}>
                  Clear
                </button>
              </p>
              <ul className="music-results" aria-label="Recent songs">
                {recent.map((t) => (
                  <SongRow key={t.id} track={t} onPick={pick} />
                ))}
              </ul>
            </>
          )}
          {!results && uploads.length > 0 && (
            <>
              <p className="hint tiny music-recent-head">Your uploads</p>
              <ul className="music-results" aria-label="Your uploads">
                {uploads.map((t) => (
                  <SongRow key={t.id} track={t} onPick={pick} onRemove={removeUpload} />
                ))}
              </ul>
            </>
          )}
        </>
      )}

      {music && (
        <div className="layer-controls">
          <p className="music-title">
            {music.cover_art ? <img className="song-art" src={coverSrc(music.cover_art, 96)} alt="" width="32" height="32" /> : "♪ "}
            <b>{music.title || "Untitled"}</b>
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
          <Slider
            label={`Duck under the ${what === "clip" ? "clip's" : "clips'"} sound`}
            value={music.duck}
            min={0}
            max={1}
            step={0.05}
            text={percentText}
            disabled={!audio}
            onChange={(v) => set({ duck: v })}
          />
          <p className="hint tiny">
            Ducking turns the song down while the {what === "clip" ? "clip's" : "clips'"} own sound is playing, and back up in the gaps
            {audio ? "." : " (tick Include audio to use it)."} You hear it in the export; the player doesn't duck.
          </p>
          {preview && (
            <>
              <label className="check">
                <input type="checkbox" checked={hear} onChange={(e) => setHear(e.target.checked)} /> Hear it in the player
              </label>
              <SongPreview music={music} relMs={relMs} clipMs={clipMs} playing={playing} hear={hear} />
            </>
          )}
        </div>
      )}

      {showClipVolume && (
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
      )}
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
