import { useEffect, useRef, useState } from "react";
import { api } from "./api.js";
import CopyButton from "./CopyButton.jsx";
import { hookUrl, refreshBazarr } from "./bazarr.js";

/** Settings -> Bazarr: where it is, the key, and the address it should call back. */
export function BazarrSettingsPanel() {
  const [url, setUrl] = useState("");
  const [key, setKey] = useState("");
  const [keySet, setKeySet] = useState(false);
  const [configured, setConfigured] = useState(false);
  const [token, setToken] = useState(null);
  const [languages, setLanguages] = useState([]);
  const [msg, setMsg] = useState(null);
  const [busy, setBusy] = useState(false);
  const hookBox = useRef(null);

  function apply(s) {
    setUrl(s.url || "");
    setKeySet(!!s.key_set);
    setConfigured(!!s.configured);
    setToken(s.hook_token || null);
    setLanguages(s.languages || []);
  }
  useEffect(() => {
    api.getBazarrSettings().then(apply).catch(() => {});
  }, []);

  function show(check) {
    if (!check) return;
    setMsg(
      check.ok
        ? {
            ok: true,
            text:
              `Connected to Bazarr ${check.version}.` +
              (check.languages?.length
                ? ` Preferred subtitle languages: ${check.languages.join(", ")}.`
                : ""),
          }
        : { ok: false, text: check.error }
    );
  }

  async function save() {
    setBusy(true);
    setMsg(null);
    try {
      const s = await api.putBazarrSettings({ url, api_key: key || undefined });
      setKey("");
      apply(s);
      show(s.check);
      refreshBazarr();
    } catch (e) {
      setMsg({ ok: false, text: e.message });
    } finally {
      setBusy(false);
    }
  }

  async function test() {
    setBusy(true);
    setMsg(null);
    try {
      show(await api.testBazarr());
      api.getBazarrSettings().then(apply);
    } catch (e) {
      setMsg({ ok: false, text: e.message });
    } finally {
      setBusy(false);
    }
  }

  async function remove() {
    if (!window.confirm("Remove the Bazarr connection?")) return;
    setBusy(true);
    try {
      apply(await api.deleteBazarrSettings());
      setMsg(null);
      refreshBazarr();
    } catch (e) {
      setMsg({ ok: false, text: e.message });
    } finally {
      setBusy(false);
    }
  }

  async function rotate() {
    if (!window.confirm("Make a new address? The one in Bazarr stops working until you replace it.")) return;
    setBusy(true);
    try {
      apply(await api.rotateBazarrHook());
    } catch (e) {
      setMsg({ ok: false, text: e.message });
    } finally {
      setBusy(false);
    }
  }

  const hook = hookUrl(typeof window !== "undefined" ? window.location.origin : "http://localhost", token);

  return (
    <div className="panel">
      <h3>Bazarr {configured && <span className="pill done">on</span>}</h3>
      <p className="hint">
        Connects to <a href="https://www.bazarr.media/" target="_blank" rel="noreferrer">Bazarr</a> so
        you can search for missing subtitles and re-time a subtitle from here, and so a subtitle it
        downloads is picked up straight away. Bazarr's own subtitle files beside the videos are read
        either way; this adds the buttons and the instant refresh.
      </p>
      <div className="field">
        <label>Bazarr address</label>
        <input
          placeholder="http://bazarr:6767"
          value={url}
          onChange={(e) => setUrl(e.target.value)}
        />
      </div>
      <div className="field">
        <label>API key</label>
        <input
          type="password"
          placeholder={keySet ? "•••••••• (saved — leave blank to keep)" : "Bazarr → Settings → General → Security"}
          value={key}
          onChange={(e) => setKey(e.target.value)}
        />
      </div>
      <div className="btn-row">
        <button type="button" onClick={save} disabled={busy || !url.trim()}>
          Save
        </button>
        {configured && (
          <>
            <button type="button" className="secondary" onClick={test} disabled={busy}>
              Test
            </button>
            <button type="button" className="danger" onClick={remove} disabled={busy}>
              Remove
            </button>
          </>
        )}
      </div>
      {msg && <p className={msg.ok ? "test ok" : "error"}>{msg.text}</p>}
      {configured && languages.length > 0 && (
        <p className="hint tiny">
          When a video has subtitles in several languages, the indexer prefers: {languages.join(", ")}.
        </p>
      )}

      {configured && token && (
        <div className="bazarr-hook">
          <h4>Instant refresh</h4>
          <p className="hint tiny">
            In Bazarr, open <b>Settings → Notifications</b>, add an Apprise URL with this address, and
            enable the "subtitle downloaded" events. This app then scans for the new subtitle a few
            seconds after Bazarr writes it, instead of at the next scheduled scan.
          </p>
          <div className="btn-row">
            <input
              ref={hookBox}
              readOnly
              value={hook}
              onFocus={(e) => e.target.select()}
              aria-label="Address for Bazarr"
            />
            <CopyButton text={hook} getTarget={() => hookBox.current} />
            <button type="button" className="secondary" onClick={rotate} disabled={busy}>
              New address
            </button>
          </div>
          <p className="hint tiny">
            If this app is reached through a different address from Bazarr (another host name or
            port), use that address here. The part after <code>hooks/bazarr/</code> is the secret.
          </p>
        </div>
      )}
    </div>
  );
}
