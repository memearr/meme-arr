import { api } from "./api.js";
import { entriesToText, formatLogLine, lastId, mergeEntries } from "./logs.js";
import { useEffect, useRef, useState } from "react";

const LOG_LEVELS = ["error", "warn", "info", "debug", "trace"];

/** Server log tail (like Sonarr's Log Files): view, filter, change the level, download. */
export function LogsPanel() {
  const [open, setOpen] = useState(false);
  const [entries, setEntries] = useState([]);
  const [minLevel, setMinLevel] = useState("info");
  const [text, setText] = useState("");
  const [paused, setPaused] = useState(false);
  const [serverLevel, setServerLevel] = useState(null);
  const [err, setErr] = useState(null);
  const bodyRef = useRef(null);
  const stick = useRef(true); // keep scrolled to the newest unless the user scrolled up

  // A changed filter starts the view over.
  useEffect(() => {
    setEntries([]);
  }, [minLevel, text]);

  useEffect(() => {
    if (!open) return undefined;
    api
      .getLogLevel()
      .then((r) => setServerLevel(r.level))
      .catch((e) => setErr(e.message));
  }, [open]);

  useEffect(() => {
    if (!open || paused) return undefined;
    let alive = true;
    let after = lastId(entries);
    const tick = () =>
      api
        .getLogs({ level: minLevel, q: text, after })
        .then((r) => {
          if (!alive) return;
          setErr(null);
          if (r.entries.length) setEntries((prev) => mergeEntries(prev, r.entries));
        })
        .catch((e) => alive && setErr(e.message));
    tick();
    const t = setInterval(() => {
      after = lastId(entriesRef.current);
      tick();
    }, 3000);
    return () => {
      alive = false;
      clearInterval(t);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, paused, minLevel, text]);

  // Latest entries for the poller's closure without restarting its interval.
  const entriesRef = useRef(entries);
  entriesRef.current = entries;

  useEffect(() => {
    const el = bodyRef.current;
    if (el && stick.current) el.scrollTop = el.scrollHeight;
  }, [entries]);

  function onScroll(e) {
    const el = e.currentTarget;
    stick.current = el.scrollHeight - el.scrollTop - el.clientHeight < 24;
  }

  async function changeServerLevel(level) {
    setErr(null);
    try {
      await api.setLogLevel(level);
      setServerLevel(level);
    } catch (e) {
      setErr(e.message);
    }
  }

  function download() {
    const url = URL.createObjectURL(new Blob([entriesToText(entries)], { type: "text/plain" }));
    const a = document.createElement("a");
    a.href = url;
    a.download = "meme-arr-logs.txt";
    a.click();
    URL.revokeObjectURL(url);
  }

  return (
    <div className="panel logs-panel">
      <div className="health-head">
        <h3>Logs</h3>
        <span className="hint tiny">recent server output, in memory — gone on restart</span>
        <button type="button" className="secondary small" onClick={() => setOpen((o) => !o)}>
          {open ? "Hide" : "Show"}
        </button>
      </div>
      {open && (
        <>
          <div className="log-controls">
            <label>
              Show
              <select value={minLevel} onChange={(e) => setMinLevel(e.target.value)}>
                {LOG_LEVELS.map((l) => (
                  <option key={l} value={l}>
                    {l}+
                  </option>
                ))}
              </select>
            </label>
            <input placeholder="Filter text…" value={text} onChange={(e) => setText(e.target.value)} />
            <label title="How verbose the server is. Debug/trace are for troubleshooting; it's remembered across restarts.">
              Server level
              <select
                value={serverLevel ?? ""}
                onChange={(e) => changeServerLevel(e.target.value)}
                disabled={serverLevel == null}
              >
                {serverLevel === "env" && <option value="env">from RUST_LOG</option>}
                {LOG_LEVELS.map((l) => (
                  <option key={l} value={l}>
                    {l}
                  </option>
                ))}
              </select>
            </label>
            <button type="button" className="secondary small" onClick={() => setPaused((p) => !p)}>
              {paused ? "Resume" : "Pause"}
            </button>
            <button type="button" className="secondary small" onClick={() => setEntries([])}>
              Clear view
            </button>
            <button type="button" className="secondary small" onClick={download} disabled={!entries.length}>
              Download
            </button>
          </div>
          {err && <p className="error">{err}</p>}
          <div className="log-tail" ref={bodyRef} onScroll={onScroll}>
            {entries.length === 0 && <div className="hint">Nothing at this level yet.</div>}
            {entries.map((e) => (
              <div key={e.id} className={`log-line ${e.level}`}>
                {formatLogLine(e)}
              </div>
            ))}
          </div>
        </>
      )}
    </div>
  );
}
