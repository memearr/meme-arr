import { ConnectionFields } from "./Settings.jsx";
import { api } from "./api.js";
import { buildMappingRows, mappingsToSave, serverPayload } from "./wizard.js";
import { useState } from "react";

const WIZARD_STEPS = ["Connect your server", "Map your libraries", "Finish"];

/** First-run guide: connect Plex, map each library folder to a mount, check, scan. */
export function SetupWizard({ onClose, onDone }) {
  const [step, setStep] = useState(0);
  const [server, setServer] = useState("plex");
  const [url, setUrl] = useState("");
  const [token, setToken] = useState("");
  const [testing, setTesting] = useState(false);
  const [libs, setLibs] = useState(null); // from a successful test
  const [err, setErr] = useState(null);
  const [dirs, setDirs] = useState([]);
  const [rows, setRows] = useState([]);
  const [saving, setSaving] = useState(false);
  const [result, setResult] = useState(null); // health report after saving

  async function testConnection() {
    setTesting(true);
    setErr(null);
    try {
      const r = await api.testSettings(serverPayload({ server, url, secret: token }));
      if (!r.ok) throw new Error(r.error || "Couldn't connect");
      setLibs(r.libraries);
    } catch (e) {
      setLibs(null);
      setErr(e.message);
    } finally {
      setTesting(false);
    }
  }

  async function goToMapping() {
    // Look one and two levels deep under the first mount root, so
    // /media/tv and /media/drive-d/TV both show up as candidates.
    let found = [];
    try {
      const top = await api.browseDirs();
      found = top.dirs;
      const deeper = await Promise.all(
        top.dirs.slice(0, 30).map((d) => api.browseDirs(d).catch(() => ({ dirs: [] })))
      );
      found = [...found, ...deeper.flatMap((x) => x.dirs)];
    } catch (_) {
      /* no browsable mount root: the user can still type paths */
    }
    setDirs(found);
    setRows(buildMappingRows(libs, found));
    setStep(1);
  }

  async function finish() {
    setSaving(true);
    setErr(null);
    try {
      await api.saveSettings({
        ...serverPayload({ server, url, secret: token }),
        mappings: mappingsToSave(rows),
      });
      // Saving triggers a background health check; re-check now and wait for it.
      const report = await api.recheckHealth().catch(() => api.health());
      setResult(report);
      setStep(2);
      onDone();
    } catch (e) {
      setErr(e.message);
    } finally {
      setSaving(false);
    }
  }

  const unmapped = rows.filter((r) => !r.container_path.trim()).length;
  const problems = (result?.issues || []).filter((i) => i.level !== "info");

  return (
    <div className="wizard-backdrop">
      <div className="panel wizard">
        <div className="wizard-head">
          <h2>Welcome to meme-arr</h2>
          <button type="button" className="link" onClick={onClose}>
            Skip — I'll use Settings
          </button>
        </div>
        <ol className="wizard-steps">
          {WIZARD_STEPS.map((s, i) => (
            <li key={s} className={i === step ? "on" : i < step ? "done" : ""}>
              {s}
            </li>
          ))}
        </ol>
        {err && <p className="error">{err}</p>}

        {step === 0 && (
          <div>
            <ConnectionFields
              server={server}
              setServer={(k) => {
                setServer(k);
                setUrl("");
                setToken("");
                setLibs(null);
              }}
              url={url}
              setUrl={setUrl}
              secret={token}
              setSecret={setToken}
              secretSet={false}
            />
            <button
              type="button"
              className="secondary"
              onClick={testConnection}
              disabled={testing || !url || !token}
            >
              {testing ? "Testing…" : "Test connection"}
            </button>
            {libs && (
              <p className="good">
                ✓ Connected — found {libs.length} librar{libs.length === 1 ? "y" : "ies"}:{" "}
                {libs.map((l) => l.title).join(", ")}
              </p>
            )}
            <div className="wizard-nav">
              <button type="button" onClick={goToMapping} disabled={!libs}>
                Next
              </button>
            </div>
          </div>
        )}

        {step === 1 && (
          <div>
            <p className="hint">
              Your media server reports where each library lives on <i>its</i> machine. Tell the app where that same
              folder is mounted <i>inside this container</i>
              {dirs.length > 0 && " (suggestions are pre-filled from what's mounted)"}.
            </p>
            <datalist id="wizard-dirs">
              {dirs.map((d) => (
                <option key={d} value={d} />
              ))}
            </datalist>
            {rows.length === 0 && <p className="hint">No movie or TV libraries found on the server.</p>}
            <table className="drill-table wizard-table">
              <thead>
                <tr>
                  <th>Library</th>
                  <th>Server folder</th>
                  <th>Folder in this container</th>
                </tr>
              </thead>
              <tbody>
                {rows.map((r, i) => (
                  <tr key={r.plex_prefix}>
                    <td>{r.library}</td>
                    <td className="mono">{r.plex_prefix}</td>
                    <td>
                      <input
                        list="wizard-dirs"
                        value={r.container_path}
                        placeholder="/media/…"
                        onChange={(e) =>
                          setRows((rs) =>
                            rs.map((x, j) => (j === i ? { ...x, container_path: e.target.value } : x))
                          )
                        }
                      />
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
            {unmapped > 0 && (
              <p className="hint">
                {unmapped} folder{unmapped === 1 ? "" : "s"} left blank won't be indexed — you can add them
                later in Settings.
              </p>
            )}
            <div className="wizard-nav">
              <button type="button" className="secondary" onClick={() => setStep(0)}>
                Back
              </button>
              <button type="button" onClick={finish} disabled={saving}>
                {saving ? "Saving & checking…" : "Save & check"}
              </button>
            </div>
          </div>
        )}

        {step === 2 && (
          <div>
            <p className="good">✓ Saved.</p>
            {problems.length === 0 ? (
              <p className="good">The health check found no problems.</p>
            ) : (
              <>
                <p>
                  The health check found {problems.length} thing{problems.length === 1 ? "" : "s"} to look
                  at:
                </p>
                <ul className="health-list">
                  {problems.map((i, k) => (
                    <li key={k} className={`health-issue ${i.level}`}>
                      <span className="health-level">{i.level}</span>
                      <div>
                        <div>{i.message}</div>
                        <div className="hint tiny">{i.fix}</div>
                      </div>
                    </li>
                  ))}
                </ul>
              </>
            )}
            <p className="hint">
              Indexing subtitles can take a while for a big library; it runs in the background.
            </p>
            <div className="wizard-nav">
              <button
                type="button"
                onClick={() => {
                  api.reindex().catch(() => {});
                  onClose();
                }}
              >
                Start indexing
              </button>
              <button type="button" className="secondary" onClick={onClose}>
                Close
              </button>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
