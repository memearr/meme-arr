import { api } from "./api.js";
import { fmtBytes, fmtRelative } from "./format.js";
import { useEffect, useState } from "react";
import { withBase } from "./base.js";

export function BackupsPanel() {
  const [data, setData] = useState(null);
  const [msg, setMsg] = useState(null); // { ok, text }
  const [busy, setBusy] = useState(false);
  const now = Math.floor(Date.now() / 1000);

  function load() {
    api
      .listBackups()
      .then(setData)
      .catch((e) => setMsg({ ok: false, text: e.message }));
  }
  useEffect(load, []);

  async function backUpNow() {
    setBusy(true);
    setMsg(null);
    try {
      const r = await api.createBackup();
      setMsg({ ok: true, text: `Saved ${r.name}.` });
      load();
    } catch (e) {
      setMsg({ ok: false, text: e.message });
    } finally {
      setBusy(false);
    }
  }

  async function onPick(e) {
    const file = e.target.files?.[0];
    e.target.value = "";
    if (!file) return;
    if (
      !window.confirm(
        `Restore settings from ${file.name}?

` +
          "This replaces your current media server connection, path mappings, login settings " +
          "(including the admin password) and task schedules with the backup's. " +
          "The subtitle index is not affected."
      )
    )
      return;
    setBusy(true);
    setMsg(null);
    try {
      await api.restoreBackup(await file.text());
      setMsg({ ok: true, text: "Settings restored. Reload the page to pick up everything." });
    } catch (err) {
      setMsg({ ok: false, text: err.message });
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="panel backups-panel">
      <div className="health-head">
        <h3>Backups</h3>
        <span className="hint tiny">
          settings only, not the index — newest {data?.keep ?? 14} kept
        </span>
        <span className="backup-actions">
          <label className="secondary small button-like">
            Restore…
            <input type="file" accept="application/json,.json" hidden onChange={onPick} disabled={busy} />
          </label>
          <button type="button" className="secondary small" onClick={backUpNow} disabled={busy}>
            {busy ? "…" : "Back up now"}
          </button>
        </span>
      </div>
      {msg && <p className={msg.ok ? "good" : "error"}>{msg.text}</p>}
      <p className="hint tiny">
        Backup files contain your media server token or API key and login secrets — keep them private.
      </p>
      {data && data.backups.length === 0 && <p className="hint">No backups yet.</p>}
      {data && data.backups.length > 0 && (
        <table className="drill-table">
          <tbody>
            {data.backups.map((b) => (
              <tr key={b.name}>
                <td>
                  <a href={withBase(`/api/backups/${b.name}`)} download={b.name}>
                    {b.name}
                  </a>
                </td>
                <td>{fmtRelative(b.modified_at, now)}</td>
                <td>{fmtBytes(b.bytes)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}
