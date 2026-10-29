import React, { useState } from "react";
import { api } from "./api.js";
import { fmtRelative } from "./format.js";
import {
  reindexView,
  requestCounts,
  requestItemTitle,
  requestStatus,
  requestedSubtitleText,
} from "./subtitles.js";
import SubtitleDiagnosis from "./SubtitleDiagnosis.jsx";

/**
 * Subtitle downloads this app has asked the media server for, and whether each
 * has become searchable yet. The server fetches a subtitle in the background,
 * so a request stays "Waiting" until its item is re-indexed and has lines;
 * this is where you re-index it (or remove the request).
 */
export default function SubtitleRequests({ requests, loading, error, onChanged, onOpen }) {
  const [busy, setBusy] = useState({}); // id -> true while re-indexing
  const [notes, setNotes] = useState({}); // id -> { ok, message } from the last re-index
  const [actionError, setActionError] = useState(null);
  const [diag, setDiag] = useState({}); // id -> true while its diagnosis is open
  const toggleDiag = (id) => setDiag((d) => ({ ...d, [id]: !d[id] }));
  const now = Math.floor(Date.now() / 1000);
  const counts = requestCounts(requests);

  async function reindex(r) {
    setBusy((b) => ({ ...b, [r.id]: true }));
    setActionError(null);
    try {
      const v = reindexView(await api.reindexItem(r.rating_key));
      setNotes((n) => ({ ...n, [r.id]: v }));
      onChanged(); // the state, and the Missing list, may have changed
    } catch (e) {
      setNotes((n) => ({ ...n, [r.id]: { ok: false, message: e.message } }));
    } finally {
      setBusy((b) => ({ ...b, [r.id]: false }));
    }
  }

  async function remove(r) {
    setActionError(null);
    try {
      await api.deleteSubtitleRequest(r.id);
      onChanged();
    } catch (e) {
      setActionError(e.message);
    }
  }

  async function clearFinished() {
    setActionError(null);
    try {
      await api.clearFinishedSubtitleRequests();
      onChanged();
    } catch (e) {
      setActionError(e.message);
    }
  }

  return (
    <div className="subreq">
      <p className="hint tiny">
        Every subtitle download you request is tracked here until it's searchable. The server fetches it
        in the background; <b>Re-index</b> looks for it (a file beside the video, or the copy Plex keeps) and indexes it.
      </p>
      <div className="btn-row">
        <button type="button" className="secondary small" onClick={onChanged} disabled={loading}>
          {loading ? "Refreshing…" : "↻ Refresh"}
        </button>
        {counts.indexed > 0 && (
          <button type="button" className="secondary small" onClick={clearFinished}>
            Clear {counts.indexed} finished
          </button>
        )}
      </div>
      {(error || actionError) && (
        <p className="error" role="alert">
          {error || actionError}
        </p>
      )}

      {requests.length === 0 && !loading && !error && (
        <p className="hint">
          No subtitle downloads requested yet. Find subtitles for an item on the Missing tab; each download
          you request shows up here.
        </p>
      )}

      {requests.length > 0 && (
        <table className="drill-table stack-table subreq-table" aria-label="Requested subtitle downloads">
          <thead>
            <tr>
              <th>Item</th>
              <th>Requested</th>
              <th>When</th>
              <th>Status</th>
              <th aria-label="Actions" />
            </tr>
          </thead>
          <tbody>
            {requests.map((r) => {
              const t = requestItemTitle(r);
              const age = Math.max(0, now - r.requested_at);
              const st = requestStatus(r, age);
              const note = notes[r.id];
              return (
                <React.Fragment key={r.id}>
                  <tr>
                    <td>
                      <div>{t.main}</div>
                      {t.sub && <div className="hint tiny">{t.sub}</div>}
                    </td>
                    <td data-label="Requested" className="subreq-what">
                      {requestedSubtitleText(r)}
                    </td>
                    <td data-label="When">{fmtRelative(r.requested_at, now)}</td>
                    <td data-label="Status">
                      <span className={`pill ${st.tone}`}>{st.label}</span>
                      {st.hint && <div className="hint tiny">{st.hint}</div>}
                    </td>
                    <td>
                      <div className="wanted-actions">
                        <button
                          type="button"
                          className="small secondary"
                          disabled={!st.canReindex || busy[r.id]}
                          onClick={() => reindex(r)}
                        >
                          {busy[r.id] ? "Indexing…" : "Re-index"}
                        </button>
                        {st.canReindex && onOpen && (
                          <button
                            type="button"
                            className="link"
                            onClick={() => onOpen({ rating_key: r.rating_key, title: r.item_title, show: r.show, season: r.season, episode: r.episode, year: r.year })}
                          >
                            Open
                          </button>
                        )}
                        <button
                          type="button"
                          className="link"
                          aria-expanded={!!diag[r.id]}
                          onClick={() => toggleDiag(r.id)}
                        >
                          {diag[r.id] ? "Hide diagnosis" : "Diagnose"}
                        </button>
                        <button type="button" className="link" onClick={() => remove(r)}>
                          Remove
                        </button>
                      </div>
                    </td>
                  </tr>
                  {note?.message && (
                    <tr className="subreq-note-row">
                      <td colSpan={5}>
                        <span className={note.ok ? "good" : "warn"} role="status">
                          {note.message}
                        </span>
                        {!note.ok && !diag[r.id] && (
                          <>
                            {" "}
                            <button type="button" className="link" onClick={() => toggleDiag(r.id)}>
                              Why not?
                            </button>
                          </>
                        )}
                      </td>
                    </tr>
                  )}
                  {diag[r.id] && (
                    <tr className="subreq-diag-row">
                      <td colSpan={5}>
                        <SubtitleDiagnosis itemId={r.rating_key} />
                      </td>
                    </tr>
                  )}
                </React.Fragment>
              );
            })}
          </tbody>
        </table>
      )}
    </div>
  );
}
