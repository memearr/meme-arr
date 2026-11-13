import { useState } from "react";
import { api } from "./api.js";

const HEADERS = [
  ["host", "Host"],
  ["x-forwarded-proto", "X-Forwarded-Proto"],
  ["x-forwarded-host", "X-Forwarded-Host"],
  ["x-forwarded-port", "X-Forwarded-Port"],
  ["x-forwarded-prefix", "X-Forwarded-Prefix"],
  ["x-forwarded-for", "X-Forwarded-For"],
  ["forwarded", "Forwarded"],
];

/** The rows to show for the answer of /api/system/connection. */
export function connectionRows(c) {
  return [
    ["Connection", c.https ? "HTTPS" : "plain HTTP"],
    ["Address the browser used", c.host || "unknown"],
    ["Path prefix added by a proxy", c.prefix || "none"],
    ["This app's address", c.app_url || "unknown"],
    ["Authentik callback", c.oidc_callback || "unknown"],
    ["Plex callback", c.plex_callback || "unknown"],
    ["Connected from", c.peer],
    ["Client address (for login limits)", c.client_ip],
    [
      "Forwarding headers",
      c.public_url
        ? "not used: PUBLIC_URL is set"
        : c.forwarded_trusted
          ? "believed (the sender is a proxy of this app)"
          : "ignored (the sender isn't a trusted proxy)",
    ],
  ];
}

/**
 * Settings: what the app concludes about the connection you are using right
 * now, so a reverse-proxy or HTTPS setup can be checked from the browser that
 * goes through it.
 */
export function ConnectionPanel() {
  const [info, setInfo] = useState(null);
  const [err, setErr] = useState(null);
  const [busy, setBusy] = useState(false);

  async function check() {
    setBusy(true);
    setErr(null);
    try {
      setInfo(await api.connectionInfo());
    } catch (e) {
      setErr(e.message);
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="panel">
      <h3>Check this connection</h3>
      <p className="hint tiny">
        Shows what this app makes of the connection you are using right now: whether it sees HTTPS, the
        address it will use for login callbacks and which client address it sees. Open this page the way
        you normally do (through your reverse proxy, if you have one) and press Check.
      </p>
      <button type="button" className="secondary" onClick={check} disabled={busy}>
        {busy ? "Checking…" : info ? "Check again" : "Check"}
      </button>
      {err && <p className="error">{err}</p>}
      {info && (
        <>
          {info.warnings?.map((w) => (
            <p key={w} className="error" role="status">
              {w}
            </p>
          ))}
          {info.warnings?.length === 0 && (
            <p className="test ok" role="status">
              Nothing looks wrong.
            </p>
          )}
          <table className="drill-table connection-table">
            <tbody>
              {connectionRows(info).map(([k, v]) => (
                <tr key={k}>
                  <th scope="row">{k}</th>
                  <td className="mono">{String(v)}</td>
                </tr>
              ))}
            </tbody>
          </table>
          <details>
            <summary className="hint tiny">Headers the app received</summary>
            <table className="drill-table connection-table">
              <tbody>
                {HEADERS.map(([key, label]) => (
                  <tr key={key}>
                    <th scope="row">{label}</th>
                    <td className="mono">{info.headers?.[key] ?? "—"}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </details>
        </>
      )}
    </div>
  );
}
