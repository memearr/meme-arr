import React, { useEffect, useState } from "react";
import { api } from "./api.js";
import { findingIcon, folderFileNote, serverStreamText, worstLevel } from "./subtitles.js";

/**
 * Why hasn't this item's requested subtitle become searchable? Asks the app
 * to compare what it can see (the video, the subtitle files beside it, what
 * the indexer would pick) with what the media server says it has, and shows
 * the likely cause in plain words, with the facts underneath.
 */
export default function SubtitleDiagnosis({ itemId }) {
  const [report, setReport] = useState(null);
  const [error, setError] = useState(null);
  const [showFacts, setShowFacts] = useState(false);

  useEffect(() => {
    let live = true;
    setReport(null);
    setError(null);
    api
      .subtitleDiagnose(itemId)
      .then((r) => live && setReport(r))
      .catch((e) => live && setError(e.message));
    return () => {
      live = false;
    };
  }, [itemId]);

  if (error) {
    return (
      <p className="error" role="alert">
        Couldn't run the diagnosis: {error}
      </p>
    );
  }
  if (!report) return <p className="hint tiny">Checking the video's folder and asking the server…</p>;

  const { local, server, findings } = report;
  const worst = worstLevel(findings);

  return (
    <div className={`diag diag-${worst}`}>
      <ul className="diag-findings" aria-label="Diagnosis">
        {findings.map((f, i) => (
          <li key={i} className={`diag-finding ${f.level}`}>
            <span className="diag-icon" aria-hidden="true">
              {findingIcon(f.level)}
            </span>
            <span>{f.text}</span>
          </li>
        ))}
      </ul>

      <button type="button" className="link" aria-expanded={showFacts} onClick={() => setShowFacts((v) => !v)}>
        {showFacts ? "Hide the details" : "Show the details"}
      </button>

      {showFacts && (
        <div className="diag-facts">
          <dl className="subsearch-details">
            <dt>Item</dt>
            <dd>{local.item_title ?? "(not in the index)"}</dd>
            <dt>Video, as the server reports it</dt>
            <dd>{local.server_path ?? "—"}</dd>
            <dt>Video, as this app looks for it</dt>
            <dd>
              {local.mapped_path ?? "—"} {local.mapped_path && (local.video_reachable ? "(found)" : "(NOT found)")}
            </dd>
            <dt>Subtitle lines indexed</dt>
            <dd>{local.indexed_lines}</dd>
          </dl>

          <h4>Subtitle files in the video's folder</h4>
          {local.files.length === 0 ? (
            <p className="hint tiny">None{local.folder ? ` in ${local.folder}` : ""}.</p>
          ) : (
            <table className="drill-table stack-table">
              <thead>
                <tr>
                  <th>File</th>
                  <th>What the indexer does with it</th>
                </tr>
              </thead>
              <tbody>
                {local.files.map((f) => (
                  <tr key={f.name}>
                    <td data-label="File">{f.name}</td>
                    <td data-label="Indexer">{folderFileNote(f)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}

          {local.other_videos_subtitles > 0 && (
            <p className="hint tiny">
              {local.other_videos_subtitles} other subtitle file{local.other_videos_subtitles === 1 ? "" : "s"} in the
              folder belong{local.other_videos_subtitles === 1 ? "s" : ""} to other videos and aren't listed.
            </p>
          )}

          <h4>What {server.kind === "jellyfin" ? "Jellyfin" : "Plex"} lists for this item</h4>
          {server.error && <p className="hint tiny">Couldn't ask: {server.error}</p>}
          {server.streams &&
            (server.streams.length === 0 ? (
              <p className="hint tiny">No subtitle streams.</p>
            ) : (
              <ul className="diag-streams">
                {server.streams.map((s, i) => (
                  <li key={i}>
                    {serverStreamText(s)}
                    {(s.path || s.key) && <div className="hint tiny">{s.path || s.key}</div>}
                  </li>
                ))}
              </ul>
            ))}
        </div>
      )}
    </div>
  );
}
