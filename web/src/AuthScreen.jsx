import { api } from "./api.js";
import { useState } from "react";
import { withBase } from "./base.js";

export function AuthScreen({ mode, onDone, authentikAvailable, plexLoginAvailable }) {
  const [password, setPassword] = useState("");
  const [confirm, setConfirm] = useState("");
  const [err, setErr] = useState(null);
  const [busy, setBusy] = useState(false);

  async function submit(e) {
    e.preventDefault();
    setErr(null);
    if (mode === "setup" && password !== confirm) {
      setErr("Passwords don't match.");
      return;
    }
    setBusy(true);
    try {
      await (mode === "setup" ? api.authSetup(password) : api.authLogin(password));
      onDone();
    } catch (e) {
      setErr(e.message);
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="shell auth-shell">
      <form className="panel auth-panel" onSubmit={submit}>
        <div className="brand">
          <span className="logo">🎬</span>
          <span className="brand-name">
            meme<span className="arr">-arr</span>
          </span>
        </div>
        <h2>{mode === "setup" ? "Set up the admin password" : "Log in"}</h2>
        {mode === "setup" && (
          <p className="hint">
            This runs once, the first time the app starts — no sign-up, just this one admin
            account.
          </p>
        )}
        <label>
          Password
          <input
            type="password"
            autoFocus
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            minLength={8}
            required
          />
        </label>
        {mode === "setup" && (
          <label>
            Confirm password
            <input
              type="password"
              value={confirm}
              onChange={(e) => setConfirm(e.target.value)}
              minLength={8}
              required
            />
          </label>
        )}
        {err && <p className="error">{err}</p>}
        <button type="submit" disabled={busy}>
          {busy ? "…" : mode === "setup" ? "Create password" : "Log in"}
        </button>
        {(authentikAvailable || plexLoginAvailable) && <div className="auth-divider">or</div>}
        {authentikAvailable && (
          <a className="auth-sso-btn" href={withBase("/api/auth/oidc/login")}>
            Sign in with Authentik
          </a>
        )}
        {plexLoginAvailable && (
          <a className="auth-sso-btn" href={withBase("/api/auth/plex/login")}>
            Sign in with Plex
          </a>
        )}
      </form>
    </div>
  );
}
