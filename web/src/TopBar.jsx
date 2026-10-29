import { useEffect, useRef, useState } from "react";
import { SEARCH_TIPS, SEARCH_TIPS_FOOTER } from "./searchTips.js";

/** A "?" next to the search box listing the operators it understands. */
function SearchTips() {
  const [open, setOpen] = useState(false);
  const box = useRef(null);

  useEffect(() => {
    if (!open) return undefined;
    // Close on a click elsewhere (not mouse-down, which can move the page under
    // the pointer and swallow the click) or Escape.
    const away = (e) => {
      if (box.current && !box.current.contains(e.target)) setOpen(false);
    };
    const esc = (e) => e.key === "Escape" && setOpen(false);
    document.addEventListener("click", away);
    document.addEventListener("keydown", esc);
    return () => {
      document.removeEventListener("click", away);
      document.removeEventListener("keydown", esc);
    };
  }, [open]);

  return (
    <div className="search-tips" ref={box}>
      <button
        type="button"
        className="secondary search-tips-btn"
        aria-label="Search tips"
        aria-expanded={open}
        title="Search tips"
        onClick={() => setOpen((o) => !o)}
      >
        ?
      </button>
      {open && (
        <div className="search-tips-pop" role="dialog" aria-label="Search tips">
          <p className="search-tips-head">Narrow a search like a web search</p>
          <dl>
            {SEARCH_TIPS.map((t) => (
              <div key={t.example} className="search-tip">
                <dt>
                  <code>{t.example}</code>
                </dt>
                <dd>{t.says}</dd>
              </div>
            ))}
          </dl>
          <p className="hint tiny">{SEARCH_TIPS_FOOTER}</p>
        </div>
      )}
    </div>
  );
}

export function TopBar({ mode, setMode, query, setQuery, onSearch, index, onReindex }) {
  const st = index?.status || {};
  return (
    <header className="topbar">
      <form className="search" onSubmit={onSearch}>
        <div className="seg">
          <button
            type="button"
            className={mode === "dialogue" ? "on" : ""}
            onClick={() => setMode("dialogue")}
          >
            Dialogue
          </button>
          <button
            type="button"
            className={mode === "title" ? "on" : ""}
            onClick={() => setMode("title")}
          >
            Title
          </button>
        </div>
        <input
          placeholder={
            mode === "dialogue"
              ? "Search a line of dialogue… e.g. “I am the one who knocks” show:\"Breaking Bad\""
              : "Search a show or movie title… e.g. pilot show:friends"
          }
          value={query}
          onChange={(e) => setQuery(e.target.value)}
        />
        <SearchTips />
        <button type="submit">Search</button>
      </form>
      <div className="topbar-actions">
        {st.running && (
          <span className="muted">
            Indexing {st.done}/{st.total}
          </span>
        )}
        <button className="secondary" onClick={onReindex} disabled={st.running}>
          ↻ Reindex
        </button>
      </div>
    </header>
  );
}
