import React, { useEffect, useRef, useState } from "react";
import { api } from "./api.js";
import SubtitleRequests from "./SubtitleRequests.jsx";
import SubtitleSearch, { CapabilityNote, useCapability } from "./SubtitleSearch.jsx";
import { LibraryFilter } from "./LibraryFilter.jsx";
import { requestCounts, unreachableNote, wantedTitle } from "./subtitles.js";
import { resolveFocus } from "./wantedFocus.js";

const PAGE = 50;

/**
 * Items with no indexed subtitles whose file we can reach: the ones a
 * subtitle download would actually fix (Sonarr's "Wanted"). Each row can run
 * the same interactive search as the editor, inline; once an item is indexed
 * it drops off the list.
 *
 * `focus` (see wantedFocus.js) is how a "no subtitles" notation elsewhere in the
 * app lands here: it pre-fills the filter and library picker and can open one
 * item's search, ready to use.
 */
export default function WantedView({ onOpen, focus, onFocusHandled }) {
  const [cap] = useCapability();
  const [filter, setFilter] = useState(() => focus?.query || "");
  const [library, setLibrary] = useState(() => focus?.libraries || []); // selected library names; empty = all
  const pendingFocus = useRef(focus || null); // resolved once the first list has loaded
  const [focusNote, setFocusNote] = useState(null);
  const [libraries, setLibraries] = useState([]);
  const [items, setItems] = useState([]);
  const [total, setTotal] = useState(0);
  const [unreachable, setUnreachable] = useState(0);
  const [loading, setLoading] = useState(false);
  const [err, setErr] = useState(null);
  const [openKey, setOpenKey] = useState(null); // row with its search expanded
  const [fixed, setFixed] = useState([]); // titles indexed this visit
  const [tab, setTab] = useState("missing"); // "missing" | "requests"
  const [reloadKey, setReloadKey] = useState(0); // bump to reload the Missing list
  const [requests, setRequests] = useState([]);
  const [reqLoading, setReqLoading] = useState(false);
  const [reqError, setReqError] = useState(null);

  function loadRequests() {
    setReqLoading(true);
    api
      .subtitleRequests()
      .then((d) => {
        setRequests(d.requests || []);
        setReqError(null);
      })
      .catch((e) => setReqError(e.message))
      .finally(() => setReqLoading(false));
  }

  useEffect(() => {
    if (focus) onFocusHandled?.(); // it has been applied (above); don't re-apply on a later visit
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Requests change on their own (an item gets indexed by a scan), so keep the
  // count fresh while this view is open.
  useEffect(() => {
    loadRequests();
    const t = setInterval(loadRequests, 30_000);
    return () => clearInterval(t);
  }, []);

  useEffect(() => {
    api
      .libraryStats()
      .then((d) => setLibraries([...new Set((d.libraries || []).map((l) => l.library))].sort()))
      .catch(() => {});
  }, []);

  // Reload from the top when the filter or library changes (debounced).
  useEffect(() => {
    const t = setTimeout(() => {
      setLoading(true);
      setErr(null);
      api
        .wanted(filter, library, 0, PAGE)
        .then((d) => {
          setItems(d.items || []);
          setTotal(d.total || 0);
          setUnreachable(d.unreachable || 0);
          if (pendingFocus.current) {
            const r = resolveFocus(pendingFocus.current, d.items || []);
            pendingFocus.current = null;
            if (r.open) setOpenKey(r.open);
            setFocusNote(r.note);
          }
        })
        .catch((e) => setErr(e.message))
        .finally(() => setLoading(false));
    }, 250);
    return () => clearTimeout(t);
  }, [filter, library, reloadKey]);

  async function loadMore() {
    setLoading(true);
    try {
      const d = await api.wanted(filter, library, items.length, PAGE);
      setItems((prev) => [...prev, ...(d.items || [])]);
      setTotal(d.total || 0);
    } catch (e) {
      setErr(e.message);
    } finally {
      setLoading(false);
    }
  }

  function gotSubtitles(item) {
    setItems((prev) => prev.filter((i) => i.rating_key !== item.rating_key));
    setTotal((t) => Math.max(0, t - 1));
    setOpenKey(null);
    setFixed((f) => [wantedTitle(item).main, ...f].slice(0, 3));
    loadRequests(); // its request is finished now
  }

  const note = unreachableNote(unreachable);
  const counts = requestCounts(requests);
  const unavailable = !!cap && !cap.supported;

  return (
    <section className="view wanted">
      <div className="view-head">
        <h2>Wanted</h2>
        <div className="seg" role="tablist" aria-label="Wanted views">
          <button
            type="button"
            role="tab"
            aria-selected={tab === "missing"}
            className={tab === "missing" ? "on" : ""}
            onClick={() => setTab("missing")}
          >
            Missing{total > 0 ? ` (${total.toLocaleString()})` : ""}
          </button>
          <button
            type="button"
            role="tab"
            aria-selected={tab === "requests"}
            className={tab === "requests" ? "on" : ""}
            onClick={() => {
              setTab("requests");
              loadRequests();
            }}
          >
            Requests{counts.total > 0 ? ` (${counts.total.toLocaleString()})` : ""}
          </button>
        </div>
      </div>
      {tab === "requests" ? (
        <SubtitleRequests
          requests={requests}
          loading={reqLoading}
          error={reqError}
          onChanged={() => {
            loadRequests();
            setReloadKey((k) => k + 1);
          }}
          onOpen={onOpen}
        />
      ) : (
        <>
          <div className="view-head wanted-filters">
            <LibraryFilter
              className="wanted-library"
              all={libraries}
              selected={library}
              onChange={setLibrary}
            />
            <input
              className="filter"
              placeholder="Filter by show, movie, or episode title…"
              value={filter}
              onChange={(e) => setFilter(e.target.value)}
            />
          </div>
      <p className="hint tiny">
        Items with no searchable subtitles, whose files this app can read. Find one online, download it, and
        index the item — it leaves this list once it has lines.
      </p>
      <CapabilityNote cap={cap} />
      {focusNote && (
        <p className="notice" role="status">
          {focusNote}
        </p>
      )}

      {fixed.length > 0 && (
        <p className="good" role="status">
          ✓ Now searchable: {fixed.join(", ")}
        </p>
      )}
      {err && <p className="error">{err}</p>}

      {items.length === 0 && !loading && !err && (
        <p className="hint">
          {filter || library.length > 0 ? "Nothing matches." : "Nothing wanted — every reachable item has subtitles."}
        </p>
      )}

      {items.length > 0 && (
        <table className="drill-table stack-table wanted-table" aria-label="Items without subtitles">
          <thead>
            <tr>
              <th>Title</th>
              <th>Library</th>
              <th aria-label="Actions" />
            </tr>
          </thead>
          <tbody>
            {items.flatMap((item) => {
              const t = wantedTitle(item);
              const open = openKey === item.rating_key;
              const rows = [
                <tr key={item.rating_key}>
                  <td>
                    <div>{t.main}</div>
                    {t.sub && <div className="hint tiny">{t.sub}</div>}
                  </td>
                  <td data-label="Library">{item.library || "—"}</td>
                  <td>
                    <div className="wanted-actions">
                    <button
                      type="button"
                      className="small secondary"
                      aria-expanded={open}
                      disabled={unavailable}
                      title={unavailable ? "Online subtitle search isn't available (see above)" : undefined}
                      onClick={() => setOpenKey(open ? null : item.rating_key)}
                    >
                      {open ? "Close" : "Find subtitles"}
                    </button>
                    <button type="button" className="link" onClick={() => onOpen(item)}>
                      Open
                    </button>
                    </div>
                  </td>
                </tr>,
              ];
              if (open) {
                rows.push(
                  <tr key={item.rating_key + ":search"} className="wanted-search-row">
                    <td colSpan={3} className="wanted-search-cell">
                      <SubtitleSearch
                        itemId={item.rating_key}
                        defaultOpen
                        hideToggle
                        onIndexed={() => gotSubtitles(item)}
                        onRequested={loadRequests}
                      />
                    </td>
                  </tr>
                );
              }
              return rows;
            })}
          </tbody>
        </table>
      )}

      {items.length < total && (
        <div className="btn-row">
          <button className="secondary" onClick={loadMore} disabled={loading}>
            {loading ? "Loading…" : `Load more (${(total - items.length).toLocaleString()} left)`}
          </button>
        </div>
      )}
      {note && <p className="hint tiny">{note}</p>}
        </>
      )}
    </section>
  );
}
