import { api } from "./api.js";
import { useEffect, useState } from "react";

export function fmtClock(ts) {
  if (!ts) return "";
  const d = new Date(ts * 1000);
  return d.toLocaleString(undefined, {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

export function ActivityView({ index, jobs }) {
  const st = index?.status || {};
  const stats = index?.stats || {};
  const pct = st.total ? Math.round((st.done / st.total) * 100) : 0;
  const [log, setLog] = useState([]);
  const [logErr, setLogErr] = useState(null);
  const [clearingLog, setClearingLog] = useState(false);

  const loadLog = () =>
    api
      .activityLog(300)
      .then((d) => setLog(d.entries || []))
      .catch((e) => setLogErr(e.message));

  useEffect(() => {
    let alive = true;
    const load = () =>
      api
        .activityLog(300)
        .then((d) => alive && setLog(d.entries || []))
        .catch((e) => alive && setLogErr(e.message));
    load();
    const t = setInterval(load, 4000);
    return () => {
      alive = false;
      clearInterval(t);
    };
  }, []);

  async function doClearLog() {
    if (!window.confirm("Clear the activity log? This cannot be undone.")) return;
    setClearingLog(true);
    try {
      await api.clearActivityLog();
      await loadLog();
    } catch (e) {
      setLogErr(e.message);
    } finally {
      setClearingLog(false);
    }
  }

  return (
    <section className="view">
      <h2>Activity</h2>
      <div className="panel">
        <h3>Indexing</h3>
        {st.running ? (
          <>
            <div className="bar">
              <div className="bar-fill" style={{ width: `${pct}%` }} />
            </div>
            <p className="muted">
              {st.done}/{st.total} ({pct}%)
              {st.current ? ` · ${st.current}` : ""}
            </p>
          </>
        ) : (
          <p className="hint">
            Idle — {stats.media_count || 0} items, {stats.cue_count || 0} lines indexed.
          </p>
        )}
      </div>

      <div className="panel">
        <h3>Log</h3>
        <div className="btn-row">
          <button
            type="button"
            className="danger"
            onClick={doClearLog}
            disabled={clearingLog || log.length === 0}
          >
            {clearingLog ? "Clearing…" : "Clear log"}
          </button>
        </div>
        {logErr && <p className="error">{logErr}</p>}
        {log.length === 0 ? (
          <p className="hint">No activity yet.</p>
        ) : (
          <ul className="log-list">
            {log.map((e, i) => (
              <li key={i} className={"log-" + e.level}>
                <span className="log-ts">{fmtClock(e.ts)}</span>
                <span className={"pill " + (e.level === "error" ? "failed" : e.level === "warn" ? "queued" : "done")}>
                  {e.level}
                </span>
                <span className="log-msg">{e.message}</span>
              </li>
            ))}
          </ul>
        )}
      </div>

      <div className="panel">
        <h3>Exports (this session)</h3>
        {jobs.length === 0 ? (
          <p className="hint">No exports yet.</p>
        ) : (
          <ul className="jobs">
            {jobs.map((j) => (
              <li key={j.id}>
                <span className={"pill " + j.status}>{j.status}</span>
                <span className="jobname">{j.filename}</span>
                {j.download_path && (
                  <a href={j.download_path} download={j.filename}>
                    download
                  </a>
                )}
                {j.error && <span className="error">{j.error}</span>}
              </li>
            ))}
          </ul>
        )}
      </div>
    </section>
  );
}
