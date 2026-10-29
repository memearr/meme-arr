import { BackupsPanel } from "./BackupsPanel.jsx";
import { LogsPanel } from "./LogsPanel.jsx";
import { SubtitleStatus } from "./SubtitleSearch.jsx";
import { TasksPanel } from "./TasksPanel.jsx";
import { api } from "./api.js";
import { fmtBytes, fmtRelative } from "./format.js";
import { fmtClock } from "./ActivityView.jsx";
import { summarizeHealth } from "./health.js";
import { useEffect, useState } from "react";
import { withBase } from "./base.js";

export function SystemView({ index, health, onHealth }) {
  const [libStats, setLibStats] = useState([]);
  const [expandMedia, setExpandMedia] = useState(false);
  const [expandSubs, setExpandSubs] = useState(false);
  const [storage, setStorage] = useState(null);
  const [storageErr, setStorageErr] = useState(null);
  const [expandStorage, setExpandStorage] = useState(false);
  const [vacuuming, setVacuuming] = useState(false);
  const [clearingCache, setClearingCache] = useState(false);
  const [clearingExports, setClearingExports] = useState(false);
  const [maintMsg, setMaintMsg] = useState(null); // { ok, text }
  const [expandExports, setExpandExports] = useState(false);
  const [exportFiles, setExportFiles] = useState(null);
  const [exportFilesErr, setExportFilesErr] = useState(null);

  function refreshStorage() {
    api
      .systemStorage()
      .then((d) => {
        setStorage(d);
        setStorageErr(null);
      })
      .catch((e) => setStorageErr(e.message));
  }

  function refreshExportFiles() {
    api
      .listExports()
      .then((d) => {
        setExportFiles(d.files || []);
        setExportFilesErr(null);
      })
      .catch((e) => setExportFilesErr(e.message));
  }

  function toggleExports() {
    setExpandExports((v) => {
      const next = !v;
      if (next && exportFiles == null) refreshExportFiles();
      return next;
    });
  }

  async function doVacuum() {
    setVacuuming(true);
    setMaintMsg(null);
    try {
      const r = await api.vacuumDatabase();
      setMaintMsg({
        ok: true,
        text: `Freed ${fmtBytes(r.saved_bytes)} in ${(r.duration_ms / 1000).toFixed(1)}s (${fmtBytes(r.before_bytes)} → ${fmtBytes(r.after_bytes)}).`,
      });
      refreshStorage();
    } catch (e) {
      setMaintMsg({ ok: false, text: e.message });
    } finally {
      setVacuuming(false);
    }
  }

  async function doClearPreviewCache() {
    setClearingCache(true);
    setMaintMsg(null);
    try {
      const r = await api.clearPreviewCache();
      setMaintMsg({ ok: true, text: `Cleared preview cache: ${fmtBytes(r.bytes)} (${r.files} files).` });
      refreshStorage();
    } catch (e) {
      setMaintMsg({ ok: false, text: e.message });
    } finally {
      setClearingCache(false);
    }
  }

  async function doClearExports() {
    if (
      !window.confirm(
        "Delete every exported meme in /output?\n\n" +
          "These are your actual finished GIFs/videos, not a cache — this cannot be undone. " +
          "Make sure you've downloaded anything you want to keep first."
      )
    )
      return;
    setClearingExports(true);
    setMaintMsg(null);
    try {
      const r = await api.clearExports();
      setMaintMsg({ ok: true, text: `Cleared exported memes: ${fmtBytes(r.bytes)} (${r.files} files).` });
      refreshStorage();
      if (exportFiles != null) refreshExportFiles();
    } catch (e) {
      setMaintMsg({ ok: false, text: e.message });
    } finally {
      setClearingExports(false);
    }
  }

  useEffect(() => {
    api
      .libraryStats()
      .then((d) => setLibStats(d.libraries || []))
      .catch(() => {});
  }, []);
  useEffect(() => {
    refreshStorage();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [index?.status?.running]); // refresh once an indexing run finishes

  const stats = index?.stats || {};
  // Group per-library/kind rows into one row per library for display.
  const byLibrary = {};
  for (const s of libStats) {
    const key = s.library;
    if (!byLibrary[key]) byLibrary[key] = { library: key, total: 0, indexed: 0, cue_count: 0, kinds: [] };
    byLibrary[key].total += s.total;
    byLibrary[key].indexed += s.indexed;
    byLibrary[key].cue_count += s.cue_count;
    byLibrary[key].kinds.push(s);
  }
  const libraries = Object.values(byLibrary).sort((a, b) => b.total - a.total);

  return (
    <section className="view">
      <h2>System</h2>
      <HealthPanel health={health} onHealth={onHealth} />
      <SubtitleStatus />
      <div className="panel">
        <div className="kv">
          <span>Health</span>
          <b className={summarizeHealth(health).level ? "bad" : health?.checked_at ? "good" : ""}>
            {summarizeHealth(health).label}
          </b>
        </div>

        <div className="kv expandable" onClick={() => setExpandMedia((v) => !v)}>
          <span>Indexed media {libraries.length > 0 && <span className="caret">{expandMedia ? "▾" : "▸"}</span>}</span>
          <b>{stats.media_count || 0}</b>
        </div>
        {expandMedia && (
          <table className="drill-table">
            <thead>
              <tr>
                <th>Library</th>
                <th>Kind</th>
                <th>Total</th>
                <th>Indexed</th>
              </tr>
            </thead>
            <tbody>
              {libStats.map((s, i) => (
                <tr key={i}>
                  <td>{s.library}</td>
                  <td>{s.kind}</td>
                  <td>{s.total.toLocaleString()}</td>
                  <td className="good">{s.indexed.toLocaleString()}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}

        <div className="kv expandable" onClick={() => setExpandSubs((v) => !v)}>
          <span>Subtitle lines {libraries.length > 0 && <span className="caret">{expandSubs ? "▾" : "▸"}</span>}</span>
          <b>{stats.cue_count || 0}</b>
        </div>
        {expandSubs && (
          <table className="drill-table">
            <thead>
              <tr>
                <th>Library</th>
                <th>Lines</th>
              </tr>
            </thead>
            <tbody>
              {libraries.map((l, i) => (
                <tr key={i}>
                  <td>{l.library}</td>
                  <td className="good">{l.cue_count.toLocaleString()}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}

        <div className="kv expandable" onClick={() => setExpandStorage((v) => !v)}>
          <span>
            Index storage {storage && <span className="caret">{expandStorage ? "▾" : "▸"}</span>}
          </span>
          <b>{storage ? fmtBytes(storage.database.total_bytes) : storageErr ? "–" : "…"}</b>
        </div>
        {storageErr && <p className="error">{storageErr}</p>}
        {expandStorage && storage && (
          <table className="drill-table">
            <tbody>
              <tr>
                <td>Database file</td>
                <td>{fmtBytes(storage.database.db_file_bytes)}</td>
              </tr>
              {storage.database.wal_bytes > 0 && (
                <tr>
                  <td>Write-ahead log</td>
                  <td>{fmtBytes(storage.database.wal_bytes)}</td>
                </tr>
              )}
              <tr>
                <td>— Subtitle dialogue text ({storage.database.cue_rows.toLocaleString()} lines)</td>
                <td>{fmtBytes(storage.database.subtitle_text_bytes)}</td>
              </tr>
              <tr>
                <td>— Media catalog ({storage.database.media_rows.toLocaleString()} items)</td>
                <td>{fmtBytes(storage.database.media_text_bytes)}</td>
              </tr>
              <tr>
                <td>— Search index &amp; overhead (FTS5, B-tree, free space)</td>
                <td>{fmtBytes(storage.database.index_overhead_bytes)}</td>
              </tr>
              <tr>
                <td>Preview cache ({storage.preview_cache.files.toLocaleString()} files)</td>
                <td>{fmtBytes(storage.preview_cache.bytes)}</td>
              </tr>
              <tr>
                <td>Exported memes ({storage.exports.files.toLocaleString()} files)</td>
                <td>{fmtBytes(storage.exports.bytes)}</td>
              </tr>
            </tbody>
          </table>
        )}
        {expandStorage && storage && (
          <div className="btn-row maint-actions">
            <button type="button" className="secondary" onClick={doVacuum} disabled={vacuuming}>
              {vacuuming ? "Vacuuming… (may take a while)" : "🧹 Vacuum & optimize database"}
            </button>
            <button
              type="button"
              className="secondary"
              onClick={doClearPreviewCache}
              disabled={clearingCache || storage.preview_cache.files === 0}
            >
              {clearingCache ? "Clearing…" : "Clear preview cache"}
            </button>
            <button
              type="button"
              className="danger"
              onClick={doClearExports}
              disabled={clearingExports || storage.exports.files === 0}
            >
              {clearingExports ? "Clearing…" : "Clear exported memes"}
            </button>
          </div>
        )}
        {expandStorage && maintMsg && (
          <p className={maintMsg.ok ? "test ok" : "error"}>{maintMsg.text}</p>
        )}
        {expandStorage && (
          <p className="hint tiny">
            Vacuum merges the search index's internal segments and compacts the database file —
            safe, but briefly needs about as much free disk space as the database itself and can
            take a while on a large library. Preview cache is safe to clear anytime (it
            regenerates on demand); exported memes are your actual deliverables — download
            anything you want to keep before clearing those.
          </p>
        )}

        <div className="kv expandable" onClick={toggleExports}>
          <span>
            Exported memes {storage && <span className="caret">{expandExports ? "▾" : "▸"}</span>}
          </span>
          <b>{storage ? storage.exports.files.toLocaleString() : "…"}</b>
        </div>
        {expandExports && (
          <>
            {exportFilesErr && <p className="error">{exportFilesErr}</p>}
            {exportFiles == null ? (
              <p className="hint">Loading…</p>
            ) : exportFiles.length === 0 ? (
              <p className="hint">Nothing exported yet.</p>
            ) : (
              <>
                <div className="btn-row">
                  <button type="button" className="secondary small" onClick={refreshExportFiles}>
                    ↻ refresh
                  </button>
                </div>
                <table className="drill-table">
                  <thead>
                    <tr>
                      <th>File</th>
                      <th>Size</th>
                      <th>Exported</th>
                      <th></th>
                    </tr>
                  </thead>
                  <tbody>
                    {exportFiles.map((f) => (
                      <tr key={f.name}>
                        <td className="mtitle">{f.name}</td>
                        <td>{fmtBytes(f.bytes)}</td>
                        <td>{fmtClock(f.modified_at)}</td>
                        <td>
                          <a
                            className="link"
                            href={withBase(`/api/download/${encodeURIComponent(f.name)}`)}
                            download={f.name}
                          >
                            download
                          </a>
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </>
            )}
          </>
        )}

        <div className="kv">
          <span>Application</span>
          <b>meme-arr</b>
        </div>
      </div>
      <TasksPanel />
      <BackupsPanel />
      <LogsPanel />
    </section>
  );
}

function HealthPanel({ health, onHealth }) {
  const [checking, setChecking] = useState(false);
  const [err, setErr] = useState(null);
  const now = Math.floor(Date.now() / 1000);
  const issues = health?.issues || [];

  async function recheck() {
    setChecking(true);
    setErr(null);
    try {
      onHealth(await api.recheckHealth());
    } catch (e) {
      setErr(e.message);
    } finally {
      setChecking(false);
    }
  }

  return (
    <div className="panel health-panel">
      <div className="health-head">
        <h3>Health</h3>
        <span className="hint tiny">
          {health?.checked_at ? `checked ${fmtRelative(health.checked_at, now)}` : "first check running…"}
        </span>
        <button
          type="button"
          className="secondary small"
          onClick={recheck}
          disabled={checking || health?.running}
        >
          {checking || health?.running ? "Checking…" : "Re-check now"}
        </button>
      </div>
      {err && <p className="error">{err}</p>}
      {health?.checked_at != null && issues.length === 0 && <p className="good">✓ No problems found.</p>}
      {issues.length > 0 && (
        <ul className="health-list">
          {issues.map((i, idx) => (
            <li key={idx} className={`health-issue ${i.level}`}>
              <span className="health-level">{i.level}</span>
              <div>
                <div>{i.message}</div>
                <div className="hint tiny">
                  {i.fix}{" "}
                  <a href={i.docs_url} target="_blank" rel="noreferrer">
                    More info ↗
                  </a>
                </div>
              </div>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
