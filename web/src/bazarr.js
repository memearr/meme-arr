// Small helpers for the Bazarr integration (settings and per-item actions).
import { useEffect, useState } from "react";
import { api } from "./api.js";
import { withBase } from "./base.js";

/**
 * The URL to give Bazarr (Settings -> Notifications, an Apprise URL) so it
 * calls us after it writes a subtitle. Apprise's `json://` is plain HTTP and
 * `jsons://` is HTTPS; the secret is the last part of the path.
 */
export function hookUrl(origin, token, base = withBase) {
  if (!token) return "";
  const u = new URL(origin);
  const scheme = u.protocol === "https:" ? "jsons" : "json";
  return `${scheme}://${u.host}${base(`/api/hooks/bazarr/${token}`)}`;
}

/** "English, French (hearing impaired)" for the languages Bazarr is missing. */
export function missingLabel(missing, names) {
  const nameOf =
    names ||
    ((code) => {
      try {
        return new Intl.DisplayNames(["en"], { type: "language" }).of(code) || code;
      } catch (_) {
        return code;
      }
    });
  return (missing || [])
    .map((m) => nameOf(m.code2) + (m.forced ? " (forced)" : "") + (m.hi ? " (hearing impaired)" : ""))
    .join(", ");
}

// Whether Bazarr is set up, asked once and shared (every Wanted row and the
// editor want to know). Saving or removing its settings calls `refreshBazarr`.
let cached = null;
const listeners = new Set();

export function refreshBazarr() {
  cached = null;
  listeners.forEach((fn) => fn());
}

export function useBazarrConfigured() {
  const [configured, setConfigured] = useState(cached);
  useEffect(() => {
    let alive = true;
    function load() {
      if (cached != null) {
        setConfigured(cached);
        return;
      }
      api
        .getBazarrSettings()
        .then((s) => {
          cached = !!s.configured;
          if (alive) setConfigured(cached);
        })
        .catch(() => alive && setConfigured(false));
    }
    listeners.add(load);
    load();
    return () => {
      alive = false;
      listeners.delete(load);
    };
  }, []);
  return configured === true;
}
