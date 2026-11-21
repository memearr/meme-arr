import { api } from "./api.js";
import { useEffect, useState } from "react";
import { LibraryFilter } from "./LibraryFilter.jsx";
import { BROWSE_STATUSES } from "./libraryFilter.js";
import { focusForItem, focusForLibrary, focusForShow } from "./wantedFocus.js";

/**
 * A clickable card that can contain real buttons (a <button> inside a <button>
 * isn't valid HTML), so a "missing" notation on it can be a link of its own.
 */
function ClickCard({ className, onClick, children }) {
  return (
    <div
      className={className}
      role="button"
      tabIndex={0}
      onClick={onClick}
      onKeyDown={(e) => {
        if (e.target === e.currentTarget && (e.key === "Enter" || e.key === " ")) {
          e.preventDefault();
          onClick();
        }
      }}
    >
      {children}
    </div>
  );
}

/** The "N missing" / "no subtitles" notation: a link to Wanted, not a dead label. */
function MissingLink({ children, onClick, title }) {
  return (
    <button
      type="button"
      className="link warn missing-link"
      title={title || "Find subtitles for these in Wanted"}
      onClick={(e) => {
        e.stopPropagation();
        onClick();
      }}
    >
      {children}
    </button>
  );
}

function pad2(n) {
  return String(n ?? 0).padStart(2, "0");
}

export function Libraries({ onOpen, onWanted }) {
  const [stats, setStats] = useState([]);
  const [openLib, setOpenLib] = useState(null);
  const [panel, setPanel] = useState("all"); // "all" | "missing"
  const [missing, setMissing] = useState([]);
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    let alive = true;
    const load = () =>
      api
        .libraryStats()
        .then((d) => alive && setStats(d.libraries || []))
        .catch(() => {});
    void load();
    const t = setInterval(load, 5000);
    return () => {
      alive = false;
      clearInterval(t);
    };
  }, []);

  async function openLibrary(name) {
    setOpenLib(name);
    setPanel("all");
    setLoading(true);
    setMissing([]);
    try {
      const d = await api.libraryMissing(name);
      setMissing(d.items || []);
    } catch (_) {
      /* ignore */
    } finally {
      setLoading(false);
    }
  }

  if (stats.length === 0) return null;

  return (
    <div className="libraries">
      <h3>Libraries</h3>
      <div className="lib-stats">
        {stats.map((s, i) => {
          const missingCount = s.no_subtitles + s.unreachable;
          const pct = s.total ? Math.round((s.indexed / s.total) * 100) : 0;
          const unit = s.kind === "movie" ? "movies" : "episodes";
          return (
            <ClickCard
              key={i}
              className={"libstat" + (openLib === s.library ? " on" : "")}
              onClick={() => openLibrary(s.library)}
            >
              <div className="libstat-head">
                <span className="libname">{s.library}</span>
                <span className="kindtag">{unit}</span>
              </div>
              <div className="libstat-count">{s.total.toLocaleString()} total</div>
              <div className="bar">
                <div className="bar-fill" style={{ width: `${pct}%` }} />
              </div>
              <div className="libstat-foot">
                <span className="good">{s.indexed.toLocaleString()} indexed</span>
                {missingCount > 0 &&
                  (onWanted ? (
                    <MissingLink onClick={() => onWanted(focusForLibrary(s.library))}>
                      {missingCount.toLocaleString()} missing
                    </MissingLink>
                  ) : (
                    <span className="warn">{missingCount.toLocaleString()} missing</span>
                  ))}
              </div>
            </ClickCard>
          );
        })}
      </div>

      {openLib && (
        <div className="panel missing-panel">
          <div className="view-head">
            <h3>{openLib}</h3>
            <div className="seg">
              <button className={panel === "all" ? "on" : ""} onClick={() => setPanel("all")}>
                All items
              </button>
              <button className={panel === "missing" ? "on" : ""} onClick={() => setPanel("missing")}>
                Missing subtitles{missing.length > 0 ? ` (${missing.length})` : ""}
              </button>
            </div>
            <button className="link" onClick={() => setOpenLib(null)}>
              close
            </button>
          </div>
          {panel === "all" ? (
            <LibraryGrid
              library={openLib}
              onOpen={onOpen}
              onWanted={onWanted}
              filterPlaceholder={`Filter within ${openLib}…`}
            />
          ) : loading ? (
            <p className="hint">Loading…</p>
          ) : missing.length === 0 ? (
            <p className="hint">Nothing missing — everything here is indexed. 🎉</p>
          ) : (
            <>
              <ul className="missing-list">
                {missing.map((m, i) => (
                  <li key={i}>
                    <span className={"pill " + (m.reason === "unreachable" ? "failed" : "queued")}>
                      {m.reason === "unreachable" ? "unreachable" : "no subs"}
                    </span>
                    <span className="mtitle">
                      {m.show ? `${m.show} · S${pad2(m.season)}E${pad2(m.episode)}` : m.title}
                    </span>
                    {onWanted && m.reason !== "unreachable" && (
                      <button
                        type="button"
                        className="link"
                        onClick={() => onWanted(focusForItem({ ...m, library: openLib }))}
                      >
                        Find subtitles
                      </button>
                    )}
                    <span className="mpath">{m.file_path}</span>
                  </li>
                ))}
              </ul>
              <p className="muted">
                <b>unreachable</b> = the file isn't on a mounted volume — check your path mappings in
                Settings. <b>no subs</b> = reachable but no text subtitle track (image-based PGS/VobSub
                subs would need OCR).
              </p>
            </>
          )}
        </div>
      )}
    </div>
  );
}

const PAGE = 60;

export function LibraryView({ onOpen, onWanted, onScope }) {
  const [names, setNames] = useState([]);
  const [libraries, setLibraries] = useState([]); // selected; empty = all
  useEffect(() => {
    api
      .libraryStats()
      .then((d) => setNames([...new Set((d.libraries || []).map((l) => l.library))].sort((a, b) => a.localeCompare(b))))
      .catch(() => {});
  }, []);
  return (
    <section className="view">
      <LibraryGrid
        title="Library"
        libraries={libraries}
        picker={<LibraryFilter all={names} selected={libraries} onChange={setLibraries} />}
        onOpen={onOpen}
        onWanted={onWanted}
        onScope={onScope}
      />
    </section>
  );
}

// Filter box + browse-all grid + pagination + show drill-down. Shared by
// the Library tab (unscoped, `library` omitted) and the Make page's
// per-library cards (`library` set), so "see more than just missing items"
// reuses the same real browse flow instead of new UI.
function LibraryGrid({ library, libraries: pickedLibraries, picker, onOpen, onWanted, onScope, title, filterPlaceholder }) {
  const [filter, setFilter] = useState("");
  const [status, setStatus] = useState("all"); // "all" | "missing" | "indexed"
  // one fixed library (a card's panel) or the picker's selection (empty = all)
  const scope = library ? [library] : pickedLibraries || [];
  const scopeKey = scope.join("\u0000");
  const [entries, setEntries] = useState([]);
  const [total, setTotal] = useState(0);
  const [offset, setOffset] = useState(0);
  const [loading, setLoading] = useState(false);
  const [err, setErr] = useState(null);
  const [openShow, setOpenShow] = useState(null); // { name } drill-down

  // Load page 0 whenever the filter (or which library we're scoped to)
  // changes (debounced).
  useEffect(() => {
    const t = setTimeout(() => {
      setLoading(true);
      setErr(null);
      api
        .libraryBrowse(filter, 0, PAGE, scope, status)
        .then((d) => {
          setEntries(d.entries || []);
          setTotal(d.total || 0);
          setOffset((d.entries || []).length);
        })
        .catch((e) => setErr(e.message))
        .finally(() => setLoading(false));
    }, 250);
    return () => clearTimeout(t);
  }, [filter, scopeKey, status]); // eslint-disable-line react-hooks/exhaustive-deps

  async function loadMore() {
    setLoading(true);
    try {
      const d = await api.libraryBrowse(filter, offset, PAGE, scope, status);
      setEntries((prev) => [...prev, ...(d.entries || [])]);
      setOffset((o) => o + (d.entries || []).length);
      setTotal(d.total || 0);
    } catch (e) {
      setErr(e.message);
    } finally {
      setLoading(false);
    }
  }

  return (
    <>
      <div className="view-head">
        {title ? (
          <h2>
            {title}{" "}
            {total > 0 && (
              <span className="muted">
                ({entries.length} of {total.toLocaleString()})
              </span>
            )}
          </h2>
        ) : (
          total > 0 && (
            <span className="muted">
              {entries.length} of {total.toLocaleString()}
            </span>
          )
        )}
        {picker}
        <div className="seg" role="group" aria-label="Subtitle status">
          {BROWSE_STATUSES.map(([key, label]) => (
            <button key={key} type="button" className={status === key ? "on" : ""} onClick={() => setStatus(key)}>
              {label}
            </button>
          ))}
        </div>
        <input
          className="filter"
          placeholder={filterPlaceholder || "Filter by show, movie, or episode title…"}
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
        />
      </div>
      {err && <p className="error">{err}</p>}
      <div className="lib-grid">
        {entries.length === 0 && !loading && (
          <p className="hint">
            {status === "missing"
              ? "Nothing is missing subtitles here. 🎉"
              : status === "indexed"
                ? "Nothing here is fully indexed yet."
                : "Nothing here yet."}
          </p>
        )}
        {entries.map((e, i) => {
          const missing = e.count - e.indexed;
          return (
            <ClickCard
              key={i}
              className="lib-card"
              onClick={() =>
                e.kind === "movie"
                  ? onOpen({ rating_key: e.rating_key, title: e.name, year: e.year, kind: "movie" })
                  : setOpenShow({ name: e.name })
              }
            >
              <div className="lib-title">
                {e.name}
                {e.kind === "movie" && e.year ? ` (${e.year})` : ""}
              </div>
              <div className="lib-sub">
                {e.kind === "show" ? (
                  <>
                    {e.count} episodes · <span className="good">{e.indexed} indexed</span>
                    {missing > 0 &&
                      (onWanted ? (
                        <>
                          {" · "}
                          <MissingLink onClick={() => onWanted(focusForShow(e.name))}>{missing} missing</MissingLink>
                        </>
                      ) : (
                        <span className="warn"> · {missing} missing</span>
                      ))}
                  </>
                ) : e.indexed > 0 ? (
                  <span className="good">indexed</span>
                ) : onWanted ? (
                  <MissingLink
                    onClick={() => onWanted(focusForItem({ rating_key: e.rating_key, title: e.name, library }))}
                  >
                    no subtitles
                  </MissingLink>
                ) : (
                  <span className="warn">no subtitles</span>
                )}
              </div>
            </ClickCard>
          );
        })}
      </div>
      {entries.length < total && (
        <div className="btn-row">
          <button className="secondary" onClick={loadMore} disabled={loading}>
            {loading ? "Loading…" : `Load more (${(total - entries.length).toLocaleString()} left)`}
          </button>
        </div>
      )}

      {openShow && (
        <ShowDrill
          show={openShow.name}
          onOpen={onOpen}
          onWanted={onWanted}
          onScope={onScope}
          onClose={() => setOpenShow(null)}
        />
      )}
    </>
  );
}

export function ShowDrill({ show, onOpen, onWanted, onScope, onClose }) {
  const [eps, setEps] = useState(null);
  useEffect(() => {
    api
      .showEpisodes(show)
      .then((d) => setEps(d.episodes || []))
      .catch(() => setEps([]));
  }, [show]);
  return (
    <div className="panel missing-panel">
      <div className="view-head">
        <h3>{show} — episodes</h3>
        {onScope && (
          <button type="button" className="secondary small" onClick={() => onScope(show)}>
            Search dialogue in this show
          </button>
        )}
        <button className="link" onClick={onClose}>
          close
        </button>
      </div>
      {eps == null ? (
        <p className="hint">Loading…</p>
      ) : (
        <ul className="missing-list">
          {eps.map((ep, i) => (
            <li key={i}>
              {ep.indexed || !onWanted ? (
                <span className={"pill " + (ep.indexed ? "done" : "queued")}>
                  {ep.indexed ? "indexed" : "no subs"}
                </span>
              ) : (
                <button
                  type="button"
                  className="pill queued pill-link"
                  title="Find subtitles for this episode in Wanted"
                  onClick={() =>
                    onWanted(
                      focusForItem({ rating_key: ep.rating_key, title: ep.title, show: ep.show, library: ep.library })
                    )
                  }
                >
                  no subs →
                </button>
              )}
              <button
                className="link ep-open"
                onClick={() =>
                  onOpen({
                    rating_key: ep.rating_key,
                    title: ep.title,
                    show: ep.show,
                    season: ep.season,
                    episode: ep.episode,
                    kind: "episode",
                  })
                }
              >
                S{pad2(ep.season)}E{pad2(ep.episode)} · {ep.title}
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
