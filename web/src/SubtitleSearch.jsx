import React, { useEffect, useState } from "react";
import { api } from "./api.js";
import {
  candidateBadges,
  capabilityView,
  detailEntries,
  emptyMessage,
  followUpMessage,
  LANGUAGES,
  loadLanguage,
  popularityText,
  reindexView,
  saveLanguage,
  sortCandidates,
} from "./subtitles.js";

/** Same text ignoring case/space (so "English" isn't shown twice). */
function sameText(a, b) {
  return !!a && !!b && a.trim().toLowerCase() === b.trim().toLowerCase();
}

/** Fetch the "can we search for subtitles" answer once. `null` while loading. */
export function useCapability(enabled = true) {
  const [cap, setCap] = useState(null);
  const [reload, setReload] = useState(0);
  useEffect(() => {
    if (!enabled) return undefined;
    let live = true;
    setCap(null);
    api
      .subtitleCapability()
      .then((c) => live && setCap(c))
      .catch((e) => live && setCap({ supported: false, reason: `Couldn't check: ${e.message}` }));
    return () => {
      live = false;
    };
  }, [enabled, reload]);
  return [cap, () => setReload((n) => n + 1)];
}

/** The capability answer as a health-style status: what it is, and how to fix it. */
export function CapabilityNote({ cap }) {
  const v = capabilityView(cap);
  return (
    <div className={`subsearch-cap ${cap ? (v.ok ? "ok" : "bad") : ""}`} role="status">
      <b>{v.title}</b>
      {v.detail && <div className="hint tiny">{v.detail}</div>}
      {v.fix && <div className="hint tiny">{v.fix}</div>}
    </div>
  );
}

/** Online subtitle status for the System page (like Sonarr's connection status). */
export function SubtitleStatus() {
  const [cap, recheck] = useCapability();
  return (
    <div className="panel subsearch-status">
      <div className="health-head">
        <h3>Online subtitle search</h3>
        <button type="button" className="secondary small" onClick={recheck} disabled={!cap}>
          {cap ? "Re-check" : "Checking…"}
        </button>
      </div>
      <CapabilityNote cap={cap} />
    </div>
  );
}

/**
 * Sonarr/Radarr-style "interactive search" for one item: pick a language,
 * search the server's subtitle sources, download the one you want. Works with
 * Plex and Jellyfin; collapsed until opened so nothing is fetched needlessly.
 */
export default function SubtitleSearch({ itemId, defaultOpen = false, hideToggle = false, onIndexed, onRequested }) {
  const [open, setOpen] = useState(defaultOpen);
  // Nothing is requested from the media server until the panel is opened.
  const [cap, recheck] = useCapability(open);
  const [language, setLanguage] = useState(() =>
    loadLanguage(typeof localStorage === "undefined" ? undefined : localStorage)
  );
  const [searching, setSearching] = useState(false);
  const [error, setError] = useState(null);
  const [found, setFound] = useState(null); // { results, note, language, server }
  const [grabs, setGrabs] = useState({}); // key -> { state: "busy"|"done"|"error", msg? }
  const [shown, setShown] = useState({}); // key -> true when its details are expanded
  const [indexed, setIndexed] = useState(null); // { pending } | { ok, done, message }

  // A different item means different results.
  useEffect(() => {
    setFound(null);
    setGrabs({});
    setShown({});
    setIndexed(null);
    setError(null);
  }, [itemId]);

  const supported = !!cap?.supported;

  async function search() {
    setSearching(true);
    setError(null);
    setFound(null);
    setGrabs({});
    setIndexed(null);
    try {
      const r = await api.subtitleSearch(itemId, language);
      setFound({ results: sortCandidates(r.results || []), note: r.note, language, server: cap?.server });
    } catch (e) {
      setError(e.message);
    } finally {
      setSearching(false);
    }
  }

  async function grab(c) {
    setGrabs((g) => ({ ...g, [c.key]: { state: "busy" } }));
    try {
      // title/provider/language only describe the request in the Requests list.
      await api.subtitleDownload(itemId, c.key, { title: c.title, provider: c.provider, language });
      setGrabs((g) => ({ ...g, [c.key]: { state: "done" } }));
      onRequested?.();
    } catch (e) {
      setGrabs((g) => ({ ...g, [c.key]: { state: "error", msg: e.message } }));
    }
  }

  // Index just this item (not the whole library) once the server has the file.
  async function indexNow() {
    setIndexed({ pending: true });
    try {
      const v = reindexView(await api.reindexItem(itemId));
      setIndexed(v);
      if (v.done) onIndexed?.();
    } catch (e) {
      setIndexed({ ok: false, done: false, message: e.message });
    }
  }

  const anyDone = Object.values(grabs).some((g) => g.state === "done");

  return (
    <div className="subsearch">
      {!hideToggle && (
        <button
          type="button"
          className="secondary"
          aria-expanded={open}
          onClick={() => setOpen((v) => !v)}
        >
          {open ? "▾" : "▸"} Find subtitles online
        </button>
      )}
      {open && (
        <div className="subsearch-body">
          {!hideToggle && <CapabilityNote cap={cap} />}
          {cap && !supported && (
            <button type="button" className="secondary small" onClick={recheck}>
              Re-check
            </button>
          )}
          {supported && (
            <>
              <div className="subsearch-form">
                <label>
                  Language
                  <select
                    value={language}
                    onChange={(e) => {
                      setLanguage(e.target.value);
                      saveLanguage(typeof localStorage === "undefined" ? undefined : localStorage, e.target.value);
                    }}
                  >
                    {LANGUAGES.map(([code, name]) => (
                      <option key={code} value={code}>
                        {name}
                      </option>
                    ))}
                  </select>
                </label>
                <button type="button" onClick={search} disabled={searching}>
                  {searching ? "Searching…" : "Search"}
                </button>
              </div>

              {error && (
                <p className="error" role="alert">
                  {error}
                </p>
              )}

              {found && found.results.length === 0 && (
                <p className="hint" role="status">
                  {emptyMessage(found.note, found.language)}
                </p>
              )}

              {found && found.results.length > 0 && (
                <table className="drill-table stack-table subsearch-table" aria-label="Subtitle search results">
                  <thead>
                    <tr>
                      <th>Subtitle</th>
                      <th>Source</th>
                      <th>Format</th>
                      <th>Flags</th>
                      <th aria-label="Download" />
                    </tr>
                  </thead>
                  <tbody>
                    {found.results.map((c) => {
                      const g = grabs[c.key];
                      const badges = candidateBadges(c, found.server);
                      const rows = detailEntries(c.details);
                      const pop = popularityText(c);
                      const open = !!shown[c.key];
                      return (
                        <React.Fragment key={c.key}>
                          <tr>
                            <td className="subsearch-title">
                              <div>{c.title}</div>
                              {c.extended_title && <div className="hint tiny">{c.extended_title}</div>}
                              <div className="hint tiny">
                                {[sameText(c.language, c.title) ? null : c.language, pop]
                                  .filter(Boolean)
                                  .join(" · ")}
                                {rows.length > 0 && (
                                  <>
                                    {((c.language && !sameText(c.language, c.title)) || pop) && " · "}
                                    <button
                                      type="button"
                                      className="link subsearch-more"
                                      aria-expanded={open}
                                      onClick={() => setShown((m) => ({ ...m, [c.key]: !m[c.key] }))}
                                    >
                                      {open ? "Hide details" : "Details"}
                                    </button>
                                  </>
                                )}
                              </div>
                            </td>
                            <td data-label="Source">{c.provider || "—"}</td>
                            <td data-label="Format">{c.format ? c.format.toUpperCase() : "—"}</td>
                            <td data-label="Flags">
                              {badges.length === 0
                                ? "—"
                                : badges.map((b) => (
                                    <span key={b} className={"pill" + (/match/i.test(b) ? " done" : "")}>
                                      {b}
                                    </span>
                                  ))}
                            </td>
                            <td>
                              {g?.state === "done" ? (
                                <span className="good">✓ Requested</span>
                              ) : (
                                <button
                                  type="button"
                                  className="small"
                                  onClick={() => grab(c)}
                                  disabled={g?.state === "busy"}
                                  aria-label={`Download ${c.title}`}
                                >
                                  {g?.state === "busy" ? "Requesting…" : g?.state === "error" ? "Retry" : "Download"}
                                </button>
                              )}
                              {g?.state === "error" && (
                                <div className="error tiny" role="alert">
                                  {g.msg}
                                </div>
                              )}
                            </td>
                          </tr>
                          {open && (
                            <tr className="subsearch-details-row">
                              <td colSpan={5}>
                                <dl className="subsearch-details">
                                  {rows.map((r) => (
                                    <React.Fragment key={r.label}>
                                      <dt>{r.label}</dt>
                                      <dd>{r.value}</dd>
                                    </React.Fragment>
                                  ))}
                                </dl>
                              </td>
                            </tr>
                          )}
                        </React.Fragment>
                      );
                    })}
                  </tbody>
                </table>
              )}

              {anyDone && (
                <div className="subsearch-followup" role="status">
                  <span>{followUpMessage(found?.server)}</span>
                  <button type="button" className="secondary small" onClick={indexNow} disabled={indexed?.pending}>
                    {indexed?.pending ? "Indexing…" : "Index this item now"}
                  </button>
                  {indexed && !indexed.pending && (
                    <span className={indexed.ok ? "good" : "warn"}>{indexed.message}</span>
                  )}
                </div>
              )}
            </>
          )}
        </div>
      )}
    </div>
  );
}
