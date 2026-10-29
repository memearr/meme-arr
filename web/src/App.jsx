import { useEffect, useState } from "react";
import { api } from "./api.js";
import { episodeLabel, fmtTime } from "./format.js";
import { summarizeHealth } from "./health.js";
import WantedView from "./WantedView.jsx";
import { ActivityView } from "./ActivityView.jsx";
import { AuthScreen } from "./AuthScreen.jsx";
import { Editor } from "./Editor.jsx";
import { Libraries, LibraryView, ShowDrill } from "./Library.jsx";
import { SequenceView } from "./SequenceView.jsx";
import { Settings } from "./Settings.jsx";
import { SetupWizard } from "./SetupWizard.jsx";
import { Sidebar } from "./Sidebar.jsx";
import { SystemView } from "./SystemView.jsx";
import { SearchScope } from "./SearchScope.jsx";
import { addFilter, buildQuery, removeFilterAt, typedOnly } from "./searchScope.js";
import { resultCountText } from "./libraryFilter.js";
import { focusForItem } from "./wantedFocus.js";
import { TopBar } from "./TopBar.jsx";
import { PADDING_MS } from "./clipSettings.js";

function newSequenceClipId() {
  return `${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
}

export default function App() {
  const [authState, setAuthState] = useState(null); // null = checking

  const checkAuth = () =>
    api
      .authStatus()
      .then(setAuthState)
      .catch(() => setAuthState({ setup_required: false, authenticated: false }));

  useEffect(() => {
    checkAuth();
    const onUnauthorized = () => setAuthState({ setup_required: false, authenticated: false });
    window.addEventListener("memearr:unauthorized", onUnauthorized);
    return () => window.removeEventListener("memearr:unauthorized", onUnauthorized);
  }, []);

  if (!authState) return null; // brief, avoids a login-screen flash on every load
  if (authState.setup_required)
    return (
      <AuthScreen
        mode="setup"
        onDone={checkAuth}
        authentikAvailable={authState.authentik_available}
        plexLoginAvailable={authState.plex_login_available}
      />
    );
  if (!authState.authenticated)
    return (
      <AuthScreen
        mode="login"
        onDone={checkAuth}
        authentikAvailable={authState.authentik_available}
        plexLoginAvailable={authState.plex_login_available}
      />
    );
  return <AppShell onLogout={() => api.authLogout().finally(checkAuth)} />;
}

function AppShell({ onLogout }) {
  const [view, setView] = useState("make");
  const [mode, setMode] = useState("dialogue");
  const [query, setQuery] = useState("");
  const [results, setResults] = useState([]);
  const [hasMore, setHasMore] = useState(false); // more matches than are listed
  const [wantedFocus, setWantedFocus] = useState(null); // where a "no subtitles" notation sent us
  const [understood, setUnderstood] = useState([]); // the operators the search box was read as
  const [listing, setListing] = useState(false); // no dialogue text: the list is items, not lines
  const [loadingMore, setLoadingMore] = useState(false);
  const [libraryNames, setLibraryNames] = useState([]); // every library, for the picker
  const [searchLibraries, setSearchLibraries] = useState([]); // searched libraries; empty = all
  const [scope, setScope] = useState([]); // filter chips (show, episode, season...) applied to every search
  const [matchingShows, setMatchingShows] = useState([]);
  const [searchError, setSearchError] = useState(null);
  const [selected, setSelected] = useState(null);
  const [index, setIndex] = useState(null);
  const [settings, setSettings] = useState(null);
  const [jobs, setJobs] = useState([]); // export jobs started this session
  const [sequence, setSequence] = useState([]); // clips queued for a multi-clip export
  const [health, setHealth] = useState(null); // GET /api/health report
  const [wizardDismissed, setWizardDismissed] = useState(
    () => sessionStorage.getItem("memearr:wizard-dismissed") === "1"
  );
  // Once the setup wizard has opened it stays until it is closed: saving the
  // settings makes the server "configured", which must not pull the wizard away
  // before its last step (the health check result and "Start indexing").
  const [wizardOpen, setWizardOpen] = useState(false);
  useEffect(() => {
    if (settings && !settings.configured && !wizardDismissed) setWizardOpen(true);
  }, [settings, wizardDismissed]);

  // Health is cached server-side (the Check health task), so polling it is cheap.
  useEffect(() => {
    let alive = true;
    const tick = () =>
      api
        .health()
        .then((h) => alive && setHealth(h))
        .catch(() => {});
    tick();
    const t = setInterval(tick, 60_000);
    return () => {
      alive = false;
      clearInterval(t);
    };
  }, []);

  // The System page shows health, so make it current when you get there (the
  // report is otherwise only re-fetched every 60s, which left a stale one up
  // after changing the media server in Settings), and follow a check that's
  // still running until it finishes.
  useEffect(() => {
    if (view !== "system") return undefined;
    let alive = true;
    const tick = () =>
      api
        .health()
        .then((h) => alive && setHealth(h))
        .catch(() => {});
    tick();
    const t = setInterval(tick, health?.running ? 2_000 : 15_000);
    return () => {
      alive = false;
      clearInterval(t);
    };
  }, [view, health?.running]);

  // Poll index status for the sidebar/top-bar badges.
  useEffect(() => {
    let alive = true;
    const tick = async () => {
      try {
        const s = await api.indexStatus();
        if (alive) setIndex(s);
      } catch (_) {
        /* ignore */
      }
    };
    tick();
    const t = setInterval(tick, 3000);
    return () => {
      alive = false;
      clearInterval(t);
    };
  }, []);

  useEffect(() => {
    api.getSettings().then(setSettings).catch(() => {});
  }, [view]);

  useEffect(() => {
    api
      .libraryStats()
      .then((d) => setLibraryNames([...new Set((d.libraries || []).map((l) => l.library))].sort()))
      .catch(() => {});
  }, [view]);

  // The text in the box plus the filter chips, which are sent as operators.
  // `over` lets a caller that has just changed a filter or the libraries search
  // with the new value, before React has applied it.
  async function doSearch(e, over = {}) {
    e?.preventDefault();
    const libraries = over.libraries ?? searchLibraries;
    const q = buildQuery(query, over.scope ?? scope);
    setSearchError(null);
    setView("make");
    if (!q) {
      if (over.clearIfEmpty) {
        setResults([]);
        setHasMore(false);
        setUnderstood([]);
        setListing(false);
        setMatchingShows([]);
      }
      return;
    }
    try {
      const run = mode === "dialogue" ? api.searchDialogue : api.searchTitle;
      const data = await run(q, { libraries });
      setResults(data.results || []);
      setHasMore(!!data.has_more);
      setUnderstood(data.filters || []);
      setListing(!!data.listing);
      setMatchingShows(data.shows || []);
    } catch (err) {
      setSearchError(err.message);
      setResults([]);
      setHasMore(false);
      setUnderstood([]);
      setListing(false);
      setMatchingShows([]);
    }
  }

  // Append the next page of the current search.
  async function showMore() {
    setLoadingMore(true);
    try {
      const run = mode === "dialogue" ? api.searchDialogue : api.searchTitle;
      const data = await run(buildQuery(query, scope), { libraries: searchLibraries, offset: results.length });
      setResults((prev) => [...prev, ...(data.results || [])]);
      setHasMore(!!data.has_more);
    } catch (err) {
      setSearchError(err.message);
    } finally {
      setLoadingMore(false);
    }
  }

  // Changing the searched libraries re-runs the search that is on screen.
  function pickSearchLibraries(libs) {
    setSearchLibraries(libs);
    doSearch(null, { libraries: libs });
  }

  // Adding or removing a filter chip re-runs the search: with some text it
  // searches the dialogue inside the narrowed set; with none it lists what
  // the filters leave.
  function changeScope(next) {
    setScope(next);
    doSearch(null, { scope: next, clearIfEmpty: true });
  }
  const addScopeFilter = (f) => changeScope(addFilter(scope, f));
  const removeScopeFilter = (i) => changeScope(removeFilterAt(scope, i));

  // "Search dialogue in this show": narrow to the show and put the cursor in the box.
  function searchWithinShow(name) {
    const next = addFilter(scope, { field: "show", value: name, negate: false });
    setScope(next);
    setView("make");
    doSearch(null, { scope: next });
    setTimeout(() => document.querySelector("form.search input")?.focus(), 80);
  }

  // A "no subtitles" / "N missing" notation was clicked: go to Wanted with it pulled up.
  function goWanted(focus) {
    setWantedFocus(focus);
    setView("wanted");
  }

  function pickResult(r) {
    // Dialogue-search hits carry a real start/end — pad and use it directly.
    // Items opened from Library/System have no timing context yet; leave
    // start_ms/end_ms unset so the Editor can pick a sensible default itself
    // (the first line of dialogue, or the middle of the runtime).
    const hasTiming = r.start_ms != null && r.end_ms != null;
    setSelected({
      rating_key: r.rating_key,
      title: r.title,
      show: r.show,
      season: r.season,
      episode: r.episode,
      year: r.year,
      line: r.text || null,
      start_ms: hasTiming ? Math.max(0, r.start_ms - PADDING_MS) : null,
      end_ms: hasTiming ? r.end_ms + PADDING_MS : null,
    });
    setView("make");
  }

  function trackJob(job) {
    setJobs((js) => {
      const i = js.findIndex((j) => j.id === job.id);
      if (i >= 0) {
        const copy = [...js];
        copy[i] = job;
        return copy;
      }
      return [job, ...js].slice(0, 25);
    });
  }

  function addToSequence(clip) {
    setSequence((seq) => [...seq, { ...clip, id: newSequenceClipId() }]);
    setView("sequence");
  }

  function dismissWizard() {
    try {
      sessionStorage.setItem("memearr:wizard-dismissed", "1");
    } catch (_) {
      /* storage unavailable: it just reappears on reload */
    }
    setWizardDismissed(true);
    setWizardOpen(false);
  }

  return (
    <div className="shell">
      {wizardOpen && (
        <SetupWizard
          onClose={dismissWizard}
          onDone={() => api.getSettings().then(setSettings).catch(() => {})}
        />
      )}
      <Sidebar
        view={view}
        setView={setView}
        index={index}
        sequenceCount={sequence.length}
        health={summarizeHealth(health)}
        onLogout={onLogout}
      />
      <div className="main">
        <TopBar
          mode={mode}
          setMode={setMode}
          query={query}
          setQuery={setQuery}
          onSearch={doSearch}
          index={index}
          onReindex={() => api.reindex().catch(() => {})}
        />
        <main className="page">
          {view === "make" && (
            <MakeView
              results={results}
              hasMore={hasMore}
              understood={understood}
              listing={listing}
              loadingMore={loadingMore}
              onShowMore={showMore}
              libraryNames={libraryNames}
              searchLibraries={searchLibraries}
              onPickLibraries={pickSearchLibraries}
              scope={scope}
              onAddFilter={addScopeFilter}
              onRemoveFilter={removeScopeFilter}
              onClearScope={() => changeScope([])}
              onScope={searchWithinShow}
              matchingShows={matchingShows}
              searchError={searchError}
              selected={selected}
              pickResult={pickResult}
              onCloseEditor={() => setSelected(null)}
              settings={settings}
              goSettings={() => setView("settings")}
              onWanted={goWanted}
              onJob={trackJob}
              onAddToSequence={addToSequence}
              query={query}
            />
          )}
          {view === "sequence" && (
            <SequenceView sequence={sequence} setSequence={setSequence} onJob={trackJob} />
          )}
          {view === "library" && <LibraryView onOpen={pickResult} onWanted={goWanted} onScope={searchWithinShow} />}
          {view === "wanted" && (
            <WantedView onOpen={pickResult} focus={wantedFocus} onFocusHandled={() => setWantedFocus(null)} />
          )}
          {view === "activity" && <ActivityView index={index} jobs={jobs} />}
          {view === "settings" && <Settings onChange={setSettings} />}
          {view === "system" && <SystemView index={index} health={health} onHealth={setHealth} />}
        </main>
      </div>
    </div>
  );
}

function MakeView({
  results,
  hasMore,
  understood,
  listing,
  loadingMore,
  onShowMore,
  libraryNames,
  searchLibraries,
  onPickLibraries,
  scope,
  onAddFilter,
  onRemoveFilter,
  onClearScope,
  onScope,
  matchingShows,
  searchError,
  selected,
  pickResult,
  onCloseEditor,
  settings,
  goSettings,
  onWanted,
  onJob,
  onAddToSequence,
  query,
}) {
  const [openShow, setOpenShow] = useState(null);

  return (
    <section className="view">
      {settings && !settings.configured && (
        <div className="notice">
          Your media server isn't connected yet.{" "}
          <button className="link" onClick={goSettings}>
            Open Settings
          </button>{" "}
          to add your server URL, credentials, and library mappings.
        </div>
      )}
      <Libraries onOpen={pickResult} onWanted={onWanted} />
      {searchError && <p className="error">{searchError}</p>}
      {matchingShows.length > 0 && (
        <div className="show-matches">
          {matchingShows.map((show) => (
            <button key={show} className="show-match" onClick={() => setOpenShow(show)}>
              <span className="pill queued">show</span>
              {show}
            </button>
          ))}
        </div>
      )}
      {openShow && (
        <ShowDrill
          show={openShow}
          onWanted={onWanted}
          onScope={(name) => {
            setOpenShow(null);
            onScope(name);
          }}
          onOpen={(ep) => {
            setOpenShow(null);
            pickResult(ep);
          }}
          onClose={() => setOpenShow(null)}
        />
      )}
      <SearchScope
        libraryNames={libraryNames}
        searchLibraries={searchLibraries}
        onPickLibraries={onPickLibraries}
        scope={scope}
        onAddFilter={onAddFilter}
        onRemoveFilter={onRemoveFilter}
        onClear={onClearScope}
      />
      {typedOnly(understood, scope).length > 0 && (
        <div className="search-understood" aria-label="How your search was read">
          <span className="muted">Typed</span>
          {typedOnly(understood, scope).map((u) => (
            <span key={u} className="pill queued">
              {u}
            </span>
          ))}
        </div>
      )}
      {listing && results.length > 0 && (
        <p className="hint tiny">
          There's no dialogue text to look for, so these are the items that match your filters. Add some
          words to search the dialogue.
        </p>
      )}
      <div className="make-body">
        <div className="results">
          {results.length === 0 && matchingShows.length === 0 && (
            <p className="hint">
              {query || scope.length > 0
                ? "No matches."
                : "Search a line of dialogue up top to find a moment to meme. Narrow it first with + Filter if you like."}
            </p>
          )}
          {results.map((r, i) => (
            <div key={i} className="result-wrap">
              <button className="result" onClick={() => pickResult(r)}>
                <div className="quote">{r.text ? `“${r.text}”` : r.title}</div>
                <div className="meta">
                  {episodeLabel(r)}
                  {r.start_ms != null && <span className="ts"> · {fmtTime(r.start_ms)}</span>}
                </div>
              </button>
              {/* a title with no indexed subtitles: the notation is a way to fix it */}
              {r.has_subtitles === false && onWanted && (
                <button
                  type="button"
                  className="pill queued pill-link result-flag"
                  title="Find subtitles for this in Wanted"
                  onClick={() => onWanted(focusForItem(r))}
                >
                  no subtitles →
                </button>
              )}
            </div>
          ))}
          {results.length > 0 && (
            <div className="results-foot">
              <span className="muted">{resultCountText(results.length, hasMore)}</span>
              {hasMore && (
                <button type="button" className="secondary" onClick={onShowMore} disabled={loadingMore}>
                  {loadingMore ? "Loading…" : "Show more"}
                </button>
              )}
            </div>
          )}
        </div>
        {selected && (
          <Editor
            key={selected.rating_key + selected.start_ms}
            item={selected}
            onJob={onJob}
            onClose={onCloseEditor}
            onAddToSequence={onAddToSequence}
          />
        )}
      </div>
    </section>
  );
}
