import { SERVERS, serverPayload } from "./wizard.js";
import { api } from "./api.js";
import { useEffect, useState } from "react";
import { withBase } from "./base.js";

function emptyMapping() {
  return { plex_prefix: "", container_path: "", label: "" };
}

/** Server picker + URL + secret, shared by Settings and the setup wizard. */
export function ConnectionFields({ server, setServer, url, setUrl, secret, setSecret, secretSet }) {
  const meta = SERVERS[server];
  return (
    <>
      <div className="field">
        <label>Media server</label>
        <select value={server} onChange={(e) => setServer(e.target.value)}>
          {Object.entries(SERVERS).map(([k, v]) => (
            <option key={k} value={k}>
              {v.label}
            </option>
          ))}
        </select>
      </div>
      <p className="hint">{meta.help}</p>
      <div className="field">
        <label>Server URL</label>
        <input placeholder={meta.urlPlaceholder} value={url} onChange={(e) => setUrl(e.target.value)} />
      </div>
      <div className="field">
        <label>{meta.secretLabel}</label>
        <input
          type="password"
          placeholder={secretSet ? "•••••••• (saved — leave blank to keep)" : meta.secretHint}
          value={secret}
          onChange={(e) => setSecret(e.target.value)}
        />
      </div>
    </>
  );
}

export function Settings({ onChange }) {
  const [server, setServer] = useState("plex");
  // Each server keeps its own URL / secret fields, so flipping the picker
  // doesn't clobber what was typed for the other one.
  const [urls, setUrls] = useState({ plex: "", jellyfin: "" });
  const [secrets, setSecrets] = useState({ plex: "", jellyfin: "" });
  const [secretsSet, setSecretsSet] = useState({ plex: false, jellyfin: false });
  const url = urls[server];
  const token = secrets[server];
  const setUrl = (v) => setUrls((u) => ({ ...u, [server]: v }));
  const setToken = (v) => setSecrets((x) => ({ ...x, [server]: v }));
  const [saveFetched, setSaveFetched] = useState(false);
  const applySaved = (s) => {
    setSaveFetched(!!s.save_fetched_subtitles);
    setServer(s.media_server === "jellyfin" ? "jellyfin" : "plex");
    setUrls({ plex: s.plex_url || "", jellyfin: s.jellyfin_url || "" });
    setSecretsSet({ plex: !!s.token_set, jellyfin: !!s.jellyfin_key_set });
  };
  const [mappings, setMappings] = useState([emptyMapping()]);
  const [test, setTest] = useState(null);
  const [testing, setTesting] = useState(false);
  const [msg, setMsg] = useState(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    api.getSettings().then((s) => {
      applySaved(s);
      setMappings(
        s.mappings && s.mappings.length
          ? s.mappings.map((m) => ({ ...m, label: m.label || "" }))
          : [emptyMapping()]
      );
    });
  }, []);

  function updateRow(i, field, value) {
    setMappings((rows) => rows.map((r, j) => (j === i ? { ...r, [field]: value } : r)));
  }
  function addRow() {
    setMappings((rows) => [...rows, emptyMapping()]);
  }
  function removeRow(i) {
    setMappings((rows) => (rows.length > 1 ? rows.filter((_, j) => j !== i) : [emptyMapping()]));
  }

  async function runTest() {
    setTesting(true);
    setTest(null);
    setMsg(null);
    try {
      const r = await api.testSettings(serverPayload({ server, url, secret: token }));
      setTest(r);
    } catch (e) {
      setTest({ ok: false, error: e.message });
    } finally {
      setTesting(false);
    }
  }

  function fillFromLibraries() {
    if (!test || !test.ok) return;
    const existing = new Set(mappings.map((m) => m.plex_prefix));
    const rows = [...mappings.filter((m) => m.plex_prefix || m.container_path)];
    (test.libraries || []).forEach((lib) => {
      (lib.locations || []).forEach((loc) => {
        if (!existing.has(loc)) {
          existing.add(loc);
          rows.push({ plex_prefix: loc, container_path: "", label: lib.title });
        }
      });
    });
    setMappings(rows.length ? rows : [emptyMapping()]);
  }

  async function save() {
    setBusy(true);
    setMsg(null);
    try {
      const payload = {
        ...serverPayload({ server, url, secret: token }),
        mappings: mappings.filter((m) => m.plex_prefix.trim() && m.container_path.trim()),
        save_fetched_subtitles: saveFetched,
      };
      const s = await api.saveSettings(payload);
      setSecrets({ plex: "", jellyfin: "" });
      applySaved(s);
      onChange?.(s);
      setMsg({ ok: true, text: "Saved." });
    } catch (e) {
      setMsg({ ok: false, text: e.message });
    } finally {
      setBusy(false);
    }
  }

  async function reindex() {
    try {
      await api.reindex();
      setMsg({ ok: true, text: "Reindex started — watch progress on the Activity tab." });
    } catch (e) {
      setMsg({ ok: false, text: e.message });
    }
  }

  async function remove() {
    if (
      !window.confirm(
        "Delete your media server URL, credentials, and all path mappings?\n\n" +
          "The app will be disconnected from your media server until you set it up again. " +
          "Your existing subtitle index is kept, but you won't be able to search or " +
          "export new clips until you reconnect."
      )
    )
      return;
    setBusy(true);
    try {
      await api.deleteSettings();
      setUrls({ plex: "", jellyfin: "" });
      setSecrets({ plex: "", jellyfin: "" });
      setSecretsSet({ plex: false, jellyfin: false });
      setServer("plex");
      setMappings([emptyMapping()]);
      setTest(null);
      const s = await api.getSettings();
      onChange?.(s);
      setMsg({ ok: true, text: "Settings deleted." });
    } catch (e) {
      setMsg({ ok: false, text: e.message });
    } finally {
      setBusy(false);
    }
  }

  return (
    <section className="view settings">
      <h2>Media server connection</h2>
      <ConnectionFields
        server={server}
        setServer={(k) => {
          setServer(k);
          setTest(null);
        }}
        url={url}
        setUrl={setUrl}
        secret={token}
        setSecret={setToken}
        secretSet={secretsSet[server]}
      />

      <div className="btn-row">
        <button type="button" onClick={runTest} disabled={testing}>
          {testing ? "Testing…" : "Test connection"}
        </button>
        <button type="button" onClick={save} disabled={busy}>
          Save
        </button>
        <button type="button" className="danger" onClick={remove} disabled={busy}>
          Delete
        </button>
      </div>

      {test && test.ok && (
        <div className="test ok">
          ✓ Connected — {test.libraries.length} librar
          {test.libraries.length === 1 ? "y" : "ies"} found:
          <ul>
            {test.libraries.map((l, i) => (
              <li key={i}>
                <b>{l.title}</b> ({l.type})
                {l.locations && l.locations.length > 0 && (
                  <span className="paths"> — {l.locations.join(", ")}</span>
                )}
              </li>
            ))}
          </ul>
          <button type="button" className="link" onClick={fillFromLibraries}>
            + Add mapping rows from these libraries
          </button>
        </div>
      )}
      {test && !test.ok && <p className="error">✕ {test.error}</p>}

      <h2>Library path mappings</h2>
      <p className="hint">
        Map the folder paths your <b>media server</b> reports (left — see a library's locations above) to where
        that same drive/share is mounted <b>in this container</b> (right — e.g.{" "}
        <code>/media/drive1</code>). Add one row per drive or library.
      </p>
      <div className="mappings">
        <div className="map-head">
          <span>Server path (prefix)</span>
          <span>Container mount path</span>
          <span>Label</span>
          <span />
        </div>
        {mappings.map((m, i) => (
          <div className="map-row" key={i}>
            <input
              placeholder="/volume1/media/Movies"
              value={m.plex_prefix}
              onChange={(e) => updateRow(i, "plex_prefix", e.target.value)}
            />
            <input
              placeholder="/media/drive1"
              value={m.container_path}
              onChange={(e) => updateRow(i, "container_path", e.target.value)}
            />
            <input
              placeholder="Movies"
              value={m.label || ""}
              onChange={(e) => updateRow(i, "label", e.target.value)}
            />
            <button type="button" className="danger small" onClick={() => removeRow(i)}>
              ✕
            </button>
          </div>
        ))}
      </div>
      {server === "plex" && (
        <>
          <h2>Subtitles Plex keeps</h2>
          <p className="hint">
            Plex stores a subtitle it downloaded in its own database, not as a file. This app reads it through Plex
            to make it searchable.
          </p>
          <label className="audio-toggle">
            <input type="checkbox" checked={saveFetched} onChange={(e) => setSaveFetched(e.target.checked)} />
            Also save it as a <code>.srt</code> file next to the video
          </label>
          <p className="hint tiny">
            Off by default. Needs the media folder mounted writable. A file with that name is never overwritten, and
            Plex may then list the subtitle twice (its own copy and the file) until you remove one.
          </p>
        </>
      )}
      <div className="btn-row">
        <button type="button" className="secondary" onClick={addRow}>
          + Add mapping
        </button>
        <button type="button" onClick={save} disabled={busy}>
          Save
        </button>
        <button type="button" className="secondary" onClick={reindex} disabled={busy}>
          Reindex now
        </button>
      </div>

      {msg && <p className={msg.ok ? "test ok" : "error"}>{msg.text}</p>}

      <ChangePasswordPanel />
      <AuthentikSettingsPanel />
      <PlexLoginPanel />
      <ApiKeyPanel />
      <TrustedProxiesPanel />
    </section>
  );
}

function TrustedProxiesPanel() {
  const [value, setValue] = useState("");
  const [msg, setMsg] = useState(null);

  useEffect(() => {
    api
      .getTrustedProxies()
      .then((r) => setValue(r.trusted_proxies))
      .catch((e) => setMsg({ ok: false, text: e.message }));
  }, []);

  async function save() {
    setMsg(null);
    try {
      await api.setTrustedProxies(value);
      setMsg({ ok: true, text: "Saved." });
    } catch (e) {
      setMsg({ ok: false, text: e.message });
    }
  }

  return (
    <div className="panel">
      <h3>Trusted proxies</h3>
      <p className="hint tiny">
        Only needed behind a reverse proxy (nginx, Traefik, Caddy, Authentik's outpost…). List its
        address — an IP or a range like <code>172.18.0.0/16</code>, comma-separated. Then failed-login
        lockouts and the Activity log use each visitor's real IP instead of lumping everyone under the
        proxy. Leave empty if you connect directly: the <code>X-Forwarded-For</code> header is
        client-controlled, so it's ignored unless the connection comes from an address listed here.
      </p>
      <input
        value={value}
        placeholder="e.g. 172.18.0.0/16, 192.168.0.5"
        onChange={(e) => setValue(e.target.value)}
      />{" "}
      <button type="button" className="secondary" onClick={save}>
        Save
      </button>
      {msg && <p className={msg.ok ? "good" : "error"}>{msg.text}</p>}
    </div>
  );
}

function ApiKeyPanel() {
  const [enabled, setEnabled] = useState(null);
  const [fresh, setFresh] = useState(null); // plaintext, shown once
  const [err, setErr] = useState(null);

  useEffect(() => {
    api
      .apiKeyStatus()
      .then((s) => setEnabled(s.enabled))
      .catch((e) => setErr(e.message));
  }, []);

  async function generate() {
    if (enabled && !window.confirm("Replace the current API key? Anything using it stops working.")) return;
    setErr(null);
    try {
      const r = await api.generateApiKey();
      setFresh(r.api_key);
      setEnabled(true);
    } catch (e) {
      setErr(e.message);
    }
  }

  async function revoke() {
    if (!window.confirm("Revoke the API key? Anything using it stops working.")) return;
    setErr(null);
    try {
      await api.revokeApiKey();
      setEnabled(false);
      setFresh(null);
    } catch (e) {
      setErr(e.message);
    }
  }

  return (
    <div className="panel">
      <h3>API key</h3>
      <p className="hint tiny">
        For dashboards and scripts (e.g. a Homepage widget). Send it as an <code>X-Api-Key</code>{" "}
        header. <b>Read-only:</b> it can only read health, tasks, index status, library stats and
        storage — never settings, backups, exports, or anything that changes state.
      </p>
      {err && <p className="error">{err}</p>}
      {fresh && (
        <div className="test ok">
          <p>Copy it now — it won't be shown again:</p>
          <code className="api-key">{fresh}</code>{" "}
          <button type="button" className="secondary small" onClick={() => navigator.clipboard?.writeText(fresh)}>
            Copy
          </button>
        </div>
      )}
      <p className="hint">{enabled == null ? "…" : enabled ? "An API key is set." : "No API key set."}</p>
      <button type="button" className="secondary" onClick={generate} disabled={enabled == null}>
        {enabled ? "Regenerate" : "Generate key"}
      </button>{" "}
      {enabled && (
        <button type="button" className="secondary" onClick={revoke}>
          Revoke
        </button>
      )}
    </div>
  );
}

function PlexLoginPanel() {
  const [linked, setLinked] = useState(false);
  const [username, setUsername] = useState(null);
  const [msg, setMsg] = useState(null);
  const [busy, setBusy] = useState(false);

  function load() {
    api.getPlexLoginSettings().then((s) => {
      setLinked(!!s.linked);
      setUsername(s.username);
    });
  }
  useEffect(load, []);

  async function unlink() {
    if (!window.confirm("Remove Plex login? You'll need to re-link to use it again.")) return;
    setBusy(true);
    setMsg(null);
    try {
      await api.deletePlexLoginSettings();
      load();
    } catch (e) {
      setMsg({ ok: false, text: e.message });
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="panel">
      <h3>Plex login {linked && <span className="pill done">on</span>}</h3>
      <p className="hint">
        Lets you log in with your Plex.tv account instead of (or alongside) the password above.
        Only the one Plex account you link here can log in this way — anyone else with a Plex
        account is refused, even if they could otherwise reach this app.
      </p>
      {linked ? (
        <p className="hint">
          Linked as <b>{username}</b>.
        </p>
      ) : (
        <p className="hint">Not linked yet.</p>
      )}
      <div className="btn-row">
        {linked ? (
          <button type="button" className="danger" onClick={unlink} disabled={busy}>
            Remove
          </button>
        ) : (
          <a className="auth-sso-btn inline" href={withBase("/api/settings/plex-login/link")}>
            Link your Plex account
          </a>
        )}
      </div>
      {msg && <p className={msg.ok ? "test ok" : "error"}>{msg.text}</p>}
    </div>
  );
}

function AuthentikSettingsPanel() {
  const [issuerUrl, setIssuerUrl] = useState("");
  const [clientId, setClientId] = useState("");
  const [clientSecret, setClientSecret] = useState("");
  const [secretSet, setSecretSet] = useState(false);
  const [configured, setConfigured] = useState(false);
  const [msg, setMsg] = useState(null);
  const [busy, setBusy] = useState(false);

  function load() {
    api.getAuthentikSettings().then((s) => {
      setIssuerUrl(s.issuer_url || "");
      setClientId(s.client_id || "");
      setSecretSet(!!s.secret_set);
      setConfigured(!!s.configured);
    });
  }
  useEffect(load, []);

  async function save() {
    setBusy(true);
    setMsg(null);
    try {
      await api.putAuthentikSettings({
        issuer_url: issuerUrl,
        client_id: clientId,
        client_secret: clientSecret || undefined,
      });
      setClientSecret("");
      setMsg({ ok: true, text: "Saved." });
      load();
    } catch (e) {
      setMsg({ ok: false, text: e.message });
    } finally {
      setBusy(false);
    }
  }

  async function remove() {
    if (!window.confirm("Remove Authentik login?")) return;
    setBusy(true);
    try {
      await api.deleteAuthentikSettings();
      setClientSecret("");
      setMsg(null);
      load();
    } catch (e) {
      setMsg({ ok: false, text: e.message });
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="panel">
      <h3>Authentik login {configured && <span className="pill done">on</span>}</h3>
      <p className="hint">
        Lets you log in with Authentik instead of (or alongside) the password above. Create an
        OAuth2/OpenID provider + application in Authentik for this app, using{" "}
        <code>{`${typeof window !== "undefined" ? window.location.origin : ""}/api/auth/oidc/callback`}</code>{" "}
        as the redirect URI.
      </p>
      <div className="field">
        <label>Issuer URL</label>
        <input
          placeholder="https://auth.example.com/application/o/meme-arr/"
          value={issuerUrl}
          onChange={(e) => setIssuerUrl(e.target.value)}
        />
      </div>
      <div className="field">
        <label>Client ID</label>
        <input value={clientId} onChange={(e) => setClientId(e.target.value)} />
      </div>
      <div className="field">
        <label>Client secret</label>
        <input
          type="password"
          placeholder={secretSet ? "•••••••• (saved — leave blank to keep)" : ""}
          value={clientSecret}
          onChange={(e) => setClientSecret(e.target.value)}
        />
      </div>
      <div className="btn-row">
        <button type="button" onClick={save} disabled={busy}>
          Save
        </button>
        {configured && (
          <button type="button" className="danger" onClick={remove} disabled={busy}>
            Remove
          </button>
        )}
      </div>
      {msg && <p className={msg.ok ? "test ok" : "error"}>{msg.text}</p>}
    </div>
  );
}

function ChangePasswordPanel() {
  const [current, setCurrent] = useState("");
  const [next, setNext] = useState("");
  const [confirm, setConfirm] = useState("");
  const [msg, setMsg] = useState(null);
  const [busy, setBusy] = useState(false);

  async function submit(e) {
    e.preventDefault();
    setMsg(null);
    if (next !== confirm) {
      setMsg({ ok: false, text: "New passwords don't match." });
      return;
    }
    setBusy(true);
    try {
      await api.authChangePassword(current, next);
      setMsg({ ok: true, text: "Password changed." });
      setCurrent("");
      setNext("");
      setConfirm("");
    } catch (e) {
      setMsg({ ok: false, text: e.message });
    } finally {
      setBusy(false);
    }
  }

  return (
    <form className="panel" onSubmit={submit}>
      <h3>Admin password</h3>
      <div className="field">
        <label>Current password</label>
        <input
          type="password"
          value={current}
          onChange={(e) => setCurrent(e.target.value)}
          required
        />
      </div>
      <div className="field">
        <label>New password</label>
        <input
          type="password"
          value={next}
          onChange={(e) => setNext(e.target.value)}
          minLength={8}
          required
        />
      </div>
      <div className="field">
        <label>Confirm new password</label>
        <input
          type="password"
          value={confirm}
          onChange={(e) => setConfirm(e.target.value)}
          minLength={8}
          required
        />
      </div>
      <div className="btn-row">
        <button type="submit" disabled={busy}>
          {busy ? "…" : "Change password"}
        </button>
      </div>
      {msg && <p className={msg.ok ? "test ok" : "error"}>{msg.text}</p>}
      <p className="hint tiny">
        Locked out? Set <code>RESET_ADMIN_PASSWORD</code> in your compose file/environment and
        restart the container — it'll reset the password to that value on startup. Remove it
        again afterward.
      </p>
    </form>
  );
}
