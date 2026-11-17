import { useEffect, useState } from "react";
import { api } from "./api.js";

/** Settings -> Navidrome: where it is and the account to use for the music under a clip. */
export function NavidromeSettingsPanel() {
  const [url, setUrl] = useState("");
  const [user, setUser] = useState("");
  const [password, setPassword] = useState("");
  const [passwordSet, setPasswordSet] = useState(false);
  const [configured, setConfigured] = useState(false);
  const [msg, setMsg] = useState(null);
  const [busy, setBusy] = useState(false);

  function apply(s) {
    setUrl(s.url || "");
    setUser(s.user || "");
    setPasswordSet(!!s.password_set);
    setConfigured(!!s.configured);
  }
  useEffect(() => {
    api.getNavidromeSettings().then(apply).catch(() => {});
  }, []);

  function show(check) {
    if (!check) return;
    setMsg(
      check.ok
        ? { ok: true, text: `Connected to Navidrome${check.version ? ` ${check.version}` : ""}.` }
        : { ok: false, text: check.error }
    );
  }

  async function run(work) {
    setBusy(true);
    setMsg(null);
    try {
      await work();
    } catch (e) {
      setMsg({ ok: false, text: e.message });
    } finally {
      setBusy(false);
    }
  }

  const save = () =>
    run(async () => {
      const s = await api.putNavidromeSettings({ url, user, password: password || undefined });
      setPassword("");
      apply(s);
      show(s.check);
    });

  const test = () =>
    run(async () => {
      show(await api.testNavidrome());
    });

  const remove = async () => {
    if (!window.confirm("Remove the Navidrome connection?")) return;
    run(async () => {
      apply(await api.deleteNavidromeSettings());
    });
  };

  return (
    <div className="panel">
      <h3>Navidrome {configured && <span className="pill done">on</span>}</h3>
      <p className="hint">
        Connects to <a href="https://www.navidrome.org/" target="_blank" rel="noreferrer">Navidrome</a>, your
        music server, so you can put one of your songs under a clip. Use an account that can play
        music; this app only searches and plays songs, it changes nothing in Navidrome.
      </p>
      <div className="field">
        <label>Navidrome address</label>
        <input placeholder="http://navidrome:4533" value={url} onChange={(e) => setUrl(e.target.value)} />
      </div>
      <div className="field">
        <label>User name</label>
        <input autoComplete="off" value={user} onChange={(e) => setUser(e.target.value)} />
      </div>
      <div className="field">
        <label>Password</label>
        <input
          type="password"
          autoComplete="new-password"
          placeholder={passwordSet ? "•••••••• (saved — leave blank to keep)" : ""}
          value={password}
          onChange={(e) => setPassword(e.target.value)}
        />
      </div>
      <div className="btn-row">
        <button type="button" onClick={save} disabled={busy || !url.trim() || !user.trim()}>
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
      {msg && (
        <p role="status" className={msg.ok ? "test ok" : "error"}>
          {msg.text}
        </p>
      )}
    </div>
  );
}
